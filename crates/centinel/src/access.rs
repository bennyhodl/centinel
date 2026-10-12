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
            || !matches!(
                request.uri().path(),
                "/workspace/documents"
                    | "/workspace/document"
                    | "/workspace/original"
                    | "/workspace/questions"
                    | "/workspace/system"
            ))
    {
        return forbidden();
    }
    request.extensions_mut().insert(role);
    next.run(request).await
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
