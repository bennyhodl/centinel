//! The private HTTP boundary and the permissions shared by HTTP and MCP.
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Admin,
}
impl Role {
    pub fn allows_op(self, name: &str) -> bool {
        self == Self::Admin || matches!(name, "search" | "read" | "list" | "download")
    }
}

pub async fn authenticate(
    State(secret): State<String>,
    mut request: Request,
    next: Next,
) -> Response {
    if request
        .headers()
        .get("x-centinel-secret")
        .and_then(|h| h.to_str().ok())
        != Some(secret.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let role = match request
        .headers()
        .get("x-centinel-role")
        .and_then(|h| h.to_str().ok())
    {
        Some("admin") => Role::Admin,
        Some("user") => Role::User,
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    if role != Role::Admin
        && request.uri().path().starts_with("/workspace/")
        && (!matches!(request.method().as_str(), "GET" | "HEAD")
            || !user_may_read(request.uri().path()))
    {
        return forbidden();
    }
    request.extensions_mut().insert(role);
    next.run(request).await
}

/// Every write under `/workspace` must come from this server's own origin: the page it
/// serves, not another site's script aimed at a loopback port. One rule on the router
/// ([`crate::http::router`]), so no handler asks and a new route cannot forget to.
///
/// Reads are open to any origin, and so is everything outside `/workspace` — the ops API
/// and MCP are authenticated by credential, not by where a browser stands.
pub async fn require_same_origin(request: Request, next: Next) -> Response {
    let write = !matches!(request.method().as_str(), "GET" | "HEAD");
    if write
        && request.uri().path().starts_with("/workspace/")
        && !crate::http::same_origin(request.headers())
    {
        return forbidden_origin();
    }
    next.run(request).await
}

fn forbidden_origin() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "error": "workspace mutations require a same-origin Origin and Host" })),
    )
        .into_response()
}

/// The `/workspace` paths a user may GET. Everything else under it is the administrator's,
/// and no user may write to any of it.
fn user_may_read(path: &str) -> bool {
    matches!(
        path,
        "/workspace/documents"
            | "/workspace/document"
            | "/workspace/original"
            | "/workspace/questions"
            | "/workspace/system"
            | "/workspace/source-hosts"
    ) || path.starts_with("/workspace/icons/")
}

pub fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "Administrator permission required"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use centinel_core::{op::Ctx, store::Store};
    use std::sync::Arc;
    use tower::ServiceExt;

    /// One rule for every write under `/workspace`, whichever handler it reaches: from
    /// another origin it is refused before the handler runs, and a read is not asked.
    #[tokio::test]
    async fn a_workspace_write_from_another_origin_is_refused_for_every_route() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let app = crate::http::router(ctx).layer(axum::Extension(Role::Admin));
        let send = |method: &str, path: &str, origin: &str| {
            let request = Request::builder()
                .method(method)
                .uri(path)
                .header("host", "127.0.0.1:8787")
                .header("origin", origin)
                .header("content-type", "application/json")
                .body(Body::from("{\"questions\":[]}"))
                .unwrap();
            app.clone().oneshot(request)
        };
        for (method, path) in [
            ("PUT", "/workspace/questions"),
            ("POST", "/workspace/jobs/anything/stop"),
            ("POST", "/workspace/sources/tampa/enabled"),
            ("POST", "/workspace/graph/host/x.com/refuse"),
        ] {
            let refused = send(method, path, "https://evil.example").await.unwrap();
            assert_eq!(refused.status(), StatusCode::FORBIDDEN, "{method} {path}");
        }
        let own = send("PUT", "/workspace/questions", "http://127.0.0.1:8787")
            .await
            .unwrap();
        assert_eq!(own.status(), StatusCode::OK);
        let read = send("GET", "/workspace/questions", "https://evil.example")
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::OK, "a read is not a write");
    }

    #[tokio::test]
    async fn private_http_requires_gateway_credentials_and_enforces_roles() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(Ctx::new(Store::open(dir.path()).await.unwrap()));
        let app = crate::http::router(ctx).layer(axum::middleware::from_fn_with_state(
            "secret".to_string(),
            authenticate,
        ));
        for (secret, role, path, method, status) in [
            ("", "admin", "/ops", "GET", 401),
            ("wrong", "admin", "/ops", "GET", 401),
            ("secret", "unknown", "/ops", "GET", 401),
            ("secret", "user", "/workspace/questions", "GET", 200),
            ("secret", "user", "/workspace/questions", "PUT", 403),
            ("secret", "user", "/workspace/runs", "POST", 403),
            ("secret", "user", "/workspace/jobs/events", "GET", 403),
            // Icons and the host each source collects from: a user sees sources in Search,
            // so may read these. A 404 is the route answering, which is past the role check.
            ("secret", "user", "/workspace/icons/tampa.gov", "GET", 404),
            ("secret", "user", "/workspace/source-hosts", "GET", 200),
            ("secret", "user", "/workspace/icons/tampa.gov", "POST", 403),
            ("secret", "user", "/workspace/icons/tampa.gov", "PUT", 403),
            ("secret", "user", "/workspace/sources", "GET", 403),
            ("secret", "user", "/ops/doctor", "POST", 403),
            ("secret", "admin", "/workspace/questions", "PUT", 200),
        ] {
            let req = Request::builder()
                .method(method)
                .uri(path)
                .header("x-centinel-secret", secret)
                .header("x-centinel-role", role)
                .header("origin", "http://127.0.0.1:8787")
                .header("host", "127.0.0.1:8787")
                .header("content-type", "application/json")
                .body(Body::from("{\"questions\":[]}"))
                .unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status().as_u16(), status, "{role} {method} {path}");
        }
        for (method, params) in [
            ("tools/list", json!({})),
            ("tools/call", json!({"name":"doctor", "arguments":{}})),
        ] {
            let req = Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("x-centinel-secret", "secret")
                .header("x-centinel-role", "user")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string(),
                ))
                .unwrap();
            let response = app.clone().oneshot(req).await.unwrap();
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1 << 20).await.unwrap())
                    .unwrap();
            if method == "tools/list" {
                let tools = body["result"]["tools"].as_array().unwrap();
                assert!(tools.iter().any(|tool| tool["name"] == "search"));
                assert!(!tools.iter().any(|tool| tool["name"] == "doctor"));
            } else {
                assert!(body.get("error").is_some());
            }
        }
    }
}
