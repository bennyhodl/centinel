//! `/workspace/icons` — the favicon beside every source and host.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/icons/{host}` | the host's icon: served from the store, else found, fetched once and kept; 404 when it has none |
//! | `GET /workspace/source-hosts` | `{ "tampa": "tampa.gov", … }`: the host each source collects from |
//!
//! ## Who may call these
//!
//! Any approved, signed-in user, because Search shows sources to people who cannot open the
//! Sources page. That is safe for the same reason the route is small: it only reads, and
//! [`centinel_core::icons`] fetches nothing for a host the store does not already know.
//!
//! `source-hosts` is its own route and not a field of `/workspace/sources`, which is an
//! administrator's: a source's definition carries its schedule and arguments. The icon needs
//! one thing from it, the host, so that is the one thing a user may read.
//!
//! ## What is served
//!
//! Another site's image from this origin, so the response is sandboxed and `nosniff`: even an
//! SVG opened directly runs nothing. The type is the one the bytes proved
//! ([`centinel_core::icons`] sniffs it), not the one the other server claimed. A week of
//! `private` caching, since the icon is kept for longer than that and only a signed-in user
//! may see it; a 404 is cached for ten minutes so a page of icon-less hosts is asked once.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use centinel_core::catalog::Sources;
use centinel_core::op::Ctx;

use super::workspace_error;

const WEEK: &str = "private, max-age=604800";
const NOT_YET: &str = "private, max-age=600";

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::get;
    axum::Router::new()
        .route("/workspace/icons/{host}", get(icon))
        .route("/workspace/source-hosts", get(source_hosts))
}

async fn icon(State(ctx): State<Arc<Ctx>>, Path(host): Path<String>) -> Response {
    match ctx.icons.get(&host).await {
        Ok(Some(icon)) => (
            [
                (header::CONTENT_TYPE, icon.content_type),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "sandbox; default-src 'none'; style-src 'unsafe-inline'".to_string(),
                ),
                (header::CACHE_CONTROL, WEEK.to_string()),
            ],
            icon.bytes,
        )
            .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, NOT_YET)]).into_response(),
        Err(error) => {
            tracing::warn!(%host, error = %format!("{error:#}"), "could not look up an icon");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn source_hosts(State(ctx): State<Arc<Ctx>>) -> Response {
    match Sources::new(&ctx.store).hosts().await {
        Ok(hosts) => Json(hosts).into_response(),
        Err(error) => workspace_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use centinel_core::config::SourceConfig;
    use centinel_core::store::Store;
    use tower::ServiceExt;

    async fn get(app: &axum::Router, uri: &str) -> Response {
        let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        app.clone().oneshot(request).await.unwrap()
    }

    /// An icon already kept comes back as the image it is, sandboxed; a host with none is a
    /// 404 the page answers with a monogram; the mapping names every source's host.
    #[tokio::test]
    async fn serves_a_kept_icon_sandboxed_and_names_each_sources_host() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov/a/b"))
            .await
            .unwrap();
        std::fs::create_dir_all(store.icons_dir()).unwrap();
        std::fs::write(store.icon_path("www.tampa.gov", "svg"), "<svg/>").unwrap();
        std::fs::write(
            store.icon_record_path("www.tampa.gov"),
            r#"{"checked":"2026-10-01T00:00:00Z","icon":{"ext":"svg","content_type":"image/svg+xml","from":"https://www.tampa.gov/m.svg"}}"#,
        )
        .unwrap();
        let app = crate::http::router(Arc::new(Ctx::new(store)));

        let found = get(&app, "/workspace/icons/www.tampa.gov").await;
        assert_eq!(found.status(), StatusCode::OK);
        let headers = found.headers().clone();
        assert_eq!(headers[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert!(headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap().starts_with("sandbox"));
        assert_eq!(headers[header::CACHE_CONTROL], WEEK);
        assert_eq!(to_bytes(found.into_body(), 1 << 20).await.unwrap().as_ref(), b"<svg/>");

        let none = get(&app, "/workspace/icons/not-a-host.example").await;
        assert_eq!(none.status(), StatusCode::NOT_FOUND);

        let hosts = get(&app, "/workspace/source-hosts").await;
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(hosts.into_body(), 1 << 20).await.unwrap()).unwrap();
        assert_eq!(body, serde_json::json!({ "tampa": "www.tampa.gov" }));
    }
}
