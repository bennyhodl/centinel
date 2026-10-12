//! Owns the private Rust listener and the supervised TanStack Start process.
use anyhow::{Context, Result, bail};
use centinel_core::op::Ctx;
use std::{path::PathBuf, process::Stdio, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

static REBUILT_SERVER: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
pub fn serve_bundle_from(bytes: Vec<u8>) {
    let _ = REBUILT_SERVER.set(bytes);
}

pub fn store_id(root: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()))
}

pub async fn serve(
    ctx: Arc<Ctx>,
    public: tokio::net::TcpListener,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let address = public.local_addr()?;
    let backend = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let backend_url = format!("http://{}", backend.local_addr()?);
    let auth_root = ctx.store.root().join("auth");
    std::fs::create_dir_all(&auth_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&auth_root, std::fs::Permissions::from_mode(0o700))?;
    }
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("generating gateway secret: {e}"))?;
    let secret: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let server = auth_root.join(format!("server-{}.mjs", crate::http::build_id()));
    std::fs::write(
        &server,
        REBUILT_SERVER
            .get()
            .map(Vec::as_slice)
            .unwrap_or(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/centinel-server.mjs"
            ))),
    )?;
    let origin = crate::http::public_url().unwrap_or_else(|| format!("http://{address}"));
    let origin_url =
        reqwest::Url::parse(&origin).context("CENTINEL_PUBLIC_URL must be an absolute URL")?;
    anyhow::ensure!(
        origin_url.path() == "/" && origin_url.query().is_none() && origin_url.fragment().is_none(),
        "CENTINEL_PUBLIC_URL must be an origin without a path"
    );
    anyhow::ensure!(
        origin_url.scheme() == "https"
            || (origin_url.scheme() == "http"
                && matches!(
                    origin_url.host_str(),
                    Some("localhost" | "127.0.0.1" | "[::1]")
                )),
        "remote authentication requires an HTTPS CENTINEL_PUBLIC_URL"
    );
    let origin = origin.trim_end_matches('/');
    let store_id = store_id(ctx.store.root());
    let store = ctx.store.clone();
    let app = crate::http::router(ctx).layer(axum::middleware::from_fn_with_state(
        secret.clone(),
        crate::access::authenticate,
    ));
    let mut backend_task = tokio::spawn(async move { axum::serve(backend, app).await });
    // The reserved public port transfers to Node. If another process takes it, Node
    // fails startup and both halves stop; no unauthenticated fallback is served.
    drop(public);
    let child = Command::new("node")
        .arg(PathBuf::from(&server))
        .env("CENTINEL_AUTH_ROOT", &auth_root)
        .env("CENTINEL_STORE_ID", store_id)
        .env("CENTINEL_BUILD_ID", crate::http::build_id())
        .env("CENTINEL_VERSION", env!("CARGO_PKG_VERSION"))
        .env("NODE_ENV", "production")
        .env("CENTINEL_PUBLIC_URL", origin)
        .env("CENTINEL_BACKEND", &backend_url)
        .env("CENTINEL_INTERNAL_SECRET", &secret)
        .env("CENTINEL_HOST", address.ip().to_string())
        .env("CENTINEL_PORT", address.port().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn();
    let result = async {
        let mut child = child.context("starting TanStack Start; install Node.js 22.13 or newer")?;
        // `Child::wait` closes stdin automatically. Keep the lifetime pipe outside
        // Child so waiting for an unexpected exit does not itself stop Start.
        let lifetime = child.stdin.take();
        let stdout = child.stdout.take().context("Start stdout missing")?;
        let mut lines = BufReader::new(stdout).lines();
        let ready = tokio::time::timeout(std::time::Duration::from_secs(30), lines.next_line()).await??;
        if ready.as_deref() != Some("centinel-start-ready") { bail!("TanStack Start failed before becoming ready; see its error above"); }
        crate::service::Running::new(address, crate::http::public_url()).write(&store)?;
        tracing::info!(url = %origin, "authenticated web and MCP server ready");
        let logs = tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await { tracing::info!(target: "centinel::start", "{line}"); }
        });
        let outcome = tokio::select! {
            status = child.wait() => Err(anyhow::anyhow!("TanStack Start exited unexpectedly: {}", status?)),
            status = &mut backend_task => Err(anyhow::anyhow!("private Rust server exited unexpectedly: {status:?}")),
            _ = shutdown => {
                drop(lifetime);
                if tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await.is_err() {
                    child.kill().await?;
                    child.wait().await?;
                }
                Ok(())
            }
        };
        logs.abort();
        outcome
    }.await;
    crate::service::Running::clear(&store);
    if !backend_task.is_finished() {
        backend_task.abort();
        let _ = backend_task.await;
    }
    let _ = std::fs::remove_file(server);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use centinel_core::store::Store;
    use serde_json::{Value, json};

    /// Exercises the shipped JS bundle, process supervision, and the real private
    /// Rust API together. In-process auth tests cannot catch a broken bundle or bridge.
    #[tokio::test]
    async fn supervised_bundle_authenticates_public_requests_and_stops_together() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        centinel_core::index::Index::open(store.index_path()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let serving = tokio::spawn(serve(Arc::new(Ctx::new(store.clone())), listener, async {
            let _ = stopped.await;
        }));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let health = format!("{origin}/health");
        for _ in 0..100 {
            if client
                .get(&health)
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            if serving.is_finished() {
                panic!("gateway exited during startup: {:?}", serving.await);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            client
                .get(&health)
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
        assert!(crate::service::Running::read(&store).is_some());
        assert_eq!(
            client
                .get(format!("{origin}/workspace/documents"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let signup = |email: &'static str| {
            client
                .post(format!("{origin}/api/auth/sign-up/email"))
                .header("origin", &origin)
                .json(
                    &json!({"name":email,"email":email,"password":"test-password-at-least-twelve"}),
                )
                .send()
        };
        let admin = signup("operator@example.com").await.unwrap();
        assert!(admin.status().is_success());
        let cookies = |response: &reqwest::Response| {
            response
                .headers()
                .get_all("set-cookie")
                .iter()
                .map(|h| h.to_str().unwrap().split(';').next().unwrap())
                .collect::<Vec<_>>()
                .join("; ")
        };
        let admin_cookie = cookies(&admin);
        let account: Value = client
            .get(format!("{origin}/api/account"))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(account["account"]["role"], "admin");
        let reader = signup("reader@example.com").await.unwrap();
        assert!(reader.status().is_success());
        let reader_cookie = cookies(&reader);
        let reader: Value = reader.json().await.unwrap();
        let approved = client
            .post(format!("{origin}/api/admin/users"))
            .header("origin", &origin)
            .header("cookie", &admin_cookie)
            .json(&json!({"id":reader["user"]["id"],"access":"approved"}))
            .send()
            .await
            .unwrap();
        assert!(approved.status().is_success());
        let corpus = client
            .get(format!("{origin}/workspace/documents"))
            .header("cookie", &reader_cookie)
            .send()
            .await
            .unwrap();
        let status = corpus.status();
        assert!(
            status.is_success(),
            "corpus returned {status}: {}",
            corpus.text().await.unwrap()
        );
        let forbidden = client
            .put(format!("{origin}/workspace/questions"))
            .header("cookie", &reader_cookie)
            .header("origin", &origin)
            .header("x-centinel-role", "admin")
            .json(&json!({"questions":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(forbidden.status(), 403);
        let mcp = client
            .post(format!("{origin}/mcp"))
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
            .send()
            .await
            .unwrap();
        assert_eq!(mcp.status(), 401);
        assert!(mcp.headers().contains_key("www-authenticate"));
        stop.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(crate::service::Running::read(&store).is_none());
        assert!(client.get(&health).send().await.is_err());
    }
}
