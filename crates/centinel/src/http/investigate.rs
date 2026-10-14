//! `/workspace/investigations` — will this site collect, and if not, why.
//!
//! | Route | Purpose |
//! |---|---|
//! | `POST /workspace/investigations` | `{ "url" }`: investigate it and check it, as a job |
//! | `GET /workspace/investigations/{id}` | what it found, once it has finished |
//!
//! ## Who may call these
//!
//! An administrator, from the browser, and for the reason [`super::sources`] gives:
//! `access.rs` refuses every `/workspace` write to a user and the gateway never forwards one
//! for an agent's token. `investigate` and `check` are still unreachable over `/ops` and MCP
//! — one makes outbound requests to an address the caller chooses, the other writes files —
//! and these routes call the library directly, for an operator.
//!
//! ## Filed, not returned
//!
//! An investigation takes tens of seconds and the person may leave the page, so it is a job
//! whose report is written under `investigations/<job>/` in the store: `report.json`, and
//! beside it the bytes `check` saved. That is what an agent is handed tomorrow to improve the
//! scraping, which is why the report carries the log of every request made
//! ([`centinel_core::requests`]). Nothing here is a Source or an Observation —
//! `investigations/` is outside the log, and deleting it loses nothing the corpus holds.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops;
use centinel_core::store::write_atomically;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{spawn_job, workspace_error};

/// Lines of extracted text an investigation keeps per document.
const PREVIEW_LINES: usize = 40;

/// What names a job this module started, and the store directory it files under.
const KIND: &str = "investigate";

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/investigations", post(start))
        .route("/workspace/investigations/{id}", get(read))
}

#[derive(Deserialize)]
struct Start {
    url: String,
}

/// Investigates an address and checks what extraction makes of it, as one job, and files
/// both reports.
async fn start(State(ctx): State<Arc<Ctx>>, Json(body): Json<Start>) -> Response {
    // An address, never a path: `check` reads local files, and this is not a file reader.
    let url = match reqwest::Url::parse(body.url.trim()) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return workspace_error(anyhow::anyhow!("`{}` is not an http(s) address", body.url)),
    };
    spawn_job(
        &ctx,
        KIND,
        url.to_string(),
        |ctx, id, progress, cancel| async move {
            let dir = ctx.store.investigation_dir(&id);
            let result = investigate(&ctx, url.as_str(), &dir, &progress, &cancel).await;
            if let Err(error) = &result {
                tracing::warn!(address = %url, error = %format!("{error:#}"), "investigation failed");
            }
            result
        },
    )
}

async fn investigate(
    ctx: &Ctx,
    url: &str,
    dir: &std::path::Path,
    progress: &Progress,
    cancel: &Cancel,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let started = jiff::Timestamp::now();

    progress.say("investigating");
    let investigate = ops::investigate(
        ctx,
        serde_json::from_value(json!({ "target": url }))?,
        progress,
        cancel,
    )
    .await;

    // No `strategy` here: the probe above is already the enumeration, and asking `check`
    // to walk the site a second time would spend up to another twenty-five requests on a
    // host nobody has vetted to learn what the first walk reported.
    progress.say("checking what extraction makes of it");
    let check = ops::check(
        ctx,
        serde_json::from_value(json!({
            "target": url,
            "out": dir.join("files"),
            "head": PREVIEW_LINES,
        }))?,
        progress,
        cancel,
    )
    .await;
    // Stopped, not finished: a report half-filled by a cancelled op is not the finding.
    cancel.check()?;

    // Each half is kept whatever became of the other: a site that refuses the probe may
    // still serve its front page, and that difference is the finding.
    let report = json!({
        "address": url,
        "started_at": started.to_string(),
        "finished_at": jiff::Timestamp::now().to_string(),
        "investigate": as_outcome(investigate),
        "check": as_outcome(check),
    });
    // Atomic, because the page polls for this file and must not read half of it.
    write_atomically(
        &dir.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}

fn as_outcome<T: serde::Serialize>(result: anyhow::Result<T>) -> Value {
    match result {
        Ok(report) => json!({ "report": report }),
        Err(error) => json!({ "error": format!("{error:#}") }),
    }
}

/// Whether `id` is a name [`start`] wrote. The id becomes a path component, so anything
/// else — `..`, a slash, another job's id — is refused before it is joined to anything.
fn is_investigation_id(id: &str) -> bool {
    id.strip_prefix(KIND)
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

async fn read(State(ctx): State<Arc<Ctx>>, Path(id): Path<String>) -> Response {
    if !is_investigation_id(&id) {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such investigation" })),
        )
            .into_response();
    }
    let path = ctx.store.investigation_dir(&id).join("report.json");
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => Json(value).into_response(),
            Err(e) => workspace_error(anyhow::anyhow!("{} does not parse: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "not finished, or no such investigation" })),
        )
            .into_response(),
        Err(e) => workspace_error(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use centinel_core::store::Store;
    use tower::ServiceExt;

    async fn app() -> (tempfile::TempDir, axum::Router) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, crate::http::router(Arc::new(Ctx::new(store))))
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// A browser on the server's own origin, which is the only caller `same_origin` admits.
    fn start_request(url: &str) -> Request<Body> {
        Request::post("/workspace/investigations")
            .header("content-type", "application/json")
            .header("host", "127.0.0.1:8787")
            .header("origin", "http://127.0.0.1:8787")
            .body(Body::from(json!({ "url": url }).to_string()))
            .unwrap()
    }

    fn get(path: &str) -> Request<Body> {
        Request::get(path).body(Body::empty()).unwrap()
    }

    /// `check` reads local files and `investigate` fetches whatever it is given, so an
    /// address that is not http(s) must never reach either.
    #[tokio::test]
    async fn only_an_http_address_starts_an_investigation() {
        for not_an_address in ["file:///etc/passwd", "/etc/passwd", "ftp://x.gov/", "x.gov"] {
            let (_dir, app) = app().await;
            let resp = app.oneshot(start_request(not_an_address)).await.unwrap();
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{not_an_address}");
            assert!(
                body_json(resp).await["error"]
                    .as_str()
                    .unwrap()
                    .contains("not an http(s) address")
            );
        }
    }

    /// A page on another origin must not be able to aim this server at a third party.
    #[tokio::test]
    async fn a_cross_origin_page_cannot_start_one() {
        let (_dir, app) = app().await;
        let request = Request::post("/workspace/investigations")
            .header("content-type", "application/json")
            .header("host", "127.0.0.1:8787")
            .header("origin", "https://evil.example")
            .body(Body::from(
                json!({ "url": "https://example.gov/" }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(request).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// The id is joined to a path. Only the shape `start` mints is read; the old `try-`
    /// names, other jobs' ids and traversal attempts are all simply not investigations.
    #[test]
    fn only_a_name_this_module_wrote_is_an_investigation() {
        assert!(is_investigation_id("investigate-1791933783859"));
        for refused in [
            "investigate-",
            "investigate-12x",
            "investigate--1",
            "try-1791933783859",
            "run-1791933783859",
            "investigate-1/../../x",
            "..",
            "",
        ] {
            assert!(!is_investigation_id(refused), "{refused:?}");
        }
    }

    #[tokio::test]
    async fn a_report_is_read_back_as_filed_and_an_unfinished_one_is_not_found() {
        let (dir, app) = app().await;

        let missing = app
            .clone()
            .oneshot(get("/workspace/investigations/investigate-1"))
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);

        let filed = Store::at(dir.path()).investigation_dir("investigate-1");
        std::fs::create_dir_all(&filed).unwrap();
        std::fs::write(
            filed.join("report.json"),
            br#"{"address":"https://x.gov/"}"#,
        )
        .unwrap();
        let found = app
            .clone()
            .oneshot(get("/workspace/investigations/investigate-1"))
            .await
            .unwrap();
        assert_eq!(found.status(), StatusCode::OK);
        assert_eq!(body_json(found).await["address"], "https://x.gov/");

        let escaped = app
            .oneshot(get("/workspace/investigations/..%2F..%2Fcentinel.db"))
            .await
            .unwrap();
        assert_eq!(escaped.status(), StatusCode::NOT_FOUND);
    }

    /// The whole route: start, follow the job to its end, read the report that was filed.
    /// Aimed at a port nothing listens on, so the site refuses everything — which is the
    /// case an investigation exists for: the report must still arrive, with the requests
    /// that were refused in it.
    #[tokio::test]
    async fn a_started_investigation_is_filed_with_the_requests_it_made() {
        let (_dir, app) = app().await;
        let started = app
            .clone()
            .oneshot(start_request("http://127.0.0.1:1/"))
            .await
            .unwrap();
        assert_eq!(started.status(), StatusCode::ACCEPTED);
        let id = body_json(started).await["id"].as_str().unwrap().to_string();
        assert!(is_investigation_id(&id), "{id}");

        let path = format!("/workspace/investigations/{id}");
        let mut report = None;
        for _ in 0..120 {
            let resp = app.clone().oneshot(get(&path)).await.unwrap();
            if resp.status() == StatusCode::OK {
                report = Some(body_json(resp).await);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        let report = report.expect("the investigation was filed within 30 s");

        assert_eq!(report["address"], "http://127.0.0.1:1/");
        let requests = report["investigate"]["report"]["requests"]
            .as_array()
            .expect("investigate survives a site that refuses everything");
        assert!(!requests.is_empty());
        assert!(
            requests.iter().all(|r| r["error"].is_string()),
            "every request was refused: {requests:?}"
        );
        assert!(
            report["check"]["error"].is_string(),
            "check failed, and says so rather than the whole report being lost"
        );
    }
}
