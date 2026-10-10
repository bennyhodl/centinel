//! `serve --tailscale` — the server on the operator's tailnet, over HTTPS.
//!
//! `serve` answers `/web` and `/mcp` on one port, so one `tailscale serve` mapping puts
//! both on the tailnet. The bind stays loopback; Tailscale terminates TLS with the
//! machine's own certificate and proxies to it. Who may connect is the tailnet's policy,
//! which is the only access control this server has (SPEC §8).
//!
//! ## Never another program's mapping
//!
//! A machine's serve config is shared by everything on it — T3 Code holds `:443` on the
//! machine this was written on — and it survives the process that wrote it. So a port is
//! taken only when it is **free** or when it already points at **this** server's local
//! address, which is what a `serve` killed before it could clean up leaves behind. 443
//! first, because a URL with no port is the one worth handing out; 8787 when 443 belongs
//! to something else; a refusal naming the holder when both do. `--tailscale-port`
//! pins one port and gets no fallback.
//!
//! Removal checks the same thing in reverse: a mapping that has since been pointed at
//! something else is left alone.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Result, bail};
use centinel_core::tool::{Tool, ToolError};
use serde_json::Value;

/// Tried in order when no port is pinned.
const PORTS: [u16; 2] = [443, 8787];

/// `tailscale serve` waits, rather than failing, when HTTPS is not enabled for the
/// tailnet — it prints a link and polls. This is how long that wait is allowed before it
/// is reported as the likely cause.
const TIMEOUT: Duration = Duration::from_secs(15);

/// Where the Mac App Store and standalone builds keep the CLI when it is not on `PATH`.
const MAC_APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

/// A mapping this process made, and everything needed to take it down again.
pub struct Published {
    /// `https://host.tailnet.ts.net`, with `:port` unless it is 443.
    pub origin: String,
    port: u16,
    /// What the mapping proxies to, exactly as `tailscale serve status` reports it.
    target: String,
}

/// Maps a tailnet HTTPS port to `local` and returns the published origin.
pub async fn publish(local: SocketAddr, pinned: Option<u16>) -> Result<Published> {
    let host = magic_dns_name().await?;
    // Tailscale rejects a bracketed IPv6 literal as a target, and `localhost` resolves
    // the way the bind did.
    let target = match local {
        SocketAddr::V4(_) => format!("http://127.0.0.1:{}", local.port()),
        SocketAddr::V6(_) => format!("http://localhost:{}", local.port()),
    };

    let config = serve_config().await?;
    let candidates = pinned.map_or(PORTS.to_vec(), |port| vec![port]);
    let mut held = Vec::new();
    let mut chosen = None;
    for port in candidates {
        match holder(&config, &host, port) {
            Some(other) if other != target => held.push(format!(":{port} → {other}")),
            _ => {
                chosen = Some(port);
                break;
            }
        }
    }
    let Some(port) = chosen else {
        bail!(
            "every tailnet port tried is already serving something else ({}); pass \
             --tailscale-port with a free one, or see `tailscale serve status`",
            held.join(", ")
        );
    };
    for taken in &held {
        tracing::info!("tailnet {taken}; using :{port}");
    }

    let https = format!("--https={port}");
    tailscale(&["serve", "--bg", &https, &target])
        .await
        .map_err(|e| explain(e, &format!("tailscale serve --bg {https} {target}")))?;

    let origin = match port {
        443 => format!("https://{host}"),
        _ => format!("https://{host}:{port}"),
    };
    tracing::info!(
        web = %format!("{origin}/web"),
        mcp = %format!("{origin}/mcp"),
        "published on the tailnet"
    );
    tracing::warn!(
        "every tailnet device your Tailscale policy lets reach this machine can read the corpus \
         and record classifier reviews — centinel has no authentication of its own (SPEC §8)"
    );
    Ok(Published {
        origin,
        port,
        target,
    })
}

/// Takes the mapping down, unless something else has claimed the port since.
///
/// Failure is a warning, not an error: it runs on the way out, and the mapping it leaves
/// points at this server's own address, so the next `serve --tailscale` reclaims it.
pub async fn unpublish(published: &Published) {
    let still_ours = match (serve_config().await, magic_dns_name().await) {
        (Ok(config), Ok(host)) => {
            holder(&config, &host, published.port).as_deref() == Some(&published.target)
        }
        _ => true,
    };
    if !still_ours {
        tracing::info!(
            port = published.port,
            "tailnet mapping now points elsewhere; leaving it"
        );
        return;
    }
    let https = format!("--https={}", published.port);
    match tailscale(&["serve", &https, "off"]).await {
        Ok(_) => tracing::info!(port = published.port, "removed from the tailnet"),
        Err(e) => tracing::warn!(error = %e, "could not remove the tailnet mapping"),
    }
}

/// This machine's MagicDNS name, without the trailing dot.
async fn magic_dns_name() -> Result<String> {
    let out = tailscale(&["status", "--json"])
        .await
        .map_err(|e| explain(e, "tailscale status"))?;
    let status: Value = serde_json::from_slice(&out)?;
    let state = status["BackendState"].as_str().unwrap_or("unknown");
    if state != "Running" {
        bail!("Tailscale is {state}, not running; run `tailscale up` and try again");
    }
    let name = status["Self"]["DNSName"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches('.');
    if name.is_empty() {
        bail!("this machine has no MagicDNS name; enable MagicDNS for the tailnet");
    }
    Ok(name.to_string())
}

async fn serve_config() -> Result<Value> {
    let out = tailscale(&["serve", "status", "--json"])
        .await
        .map_err(|e| explain(e, "tailscale serve status"))?;
    // Nothing served prints nothing at all on some versions.
    if out.iter().all(u8::is_ascii_whitespace) {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_slice(&out)?)
}

/// What `port` on the tailnet currently serves, or `None` when it serves nothing.
///
/// A single proxy at `/` is reported as its target, so it can be compared with ours;
/// anything else — more handlers, a raw TCP forward — is described, never reclaimed.
fn holder(config: &Value, host: &str, port: u16) -> Option<String> {
    let handlers = &config["Web"][format!("{host}:{port}")]["Handlers"];
    if let Some(handlers) = handlers.as_object() {
        return match (
            handlers.len(),
            handlers.get("/").and_then(|h| h["Proxy"].as_str()),
        ) {
            (1, Some(proxy)) => Some(proxy.to_string()),
            _ => Some(format!("{} handlers", handlers.len())),
        };
    }
    config["TCP"]
        .get(port.to_string())
        .map(|tcp| match tcp["TCPForward"].as_str() {
            Some(to) => format!("a TCP forward to {to}"),
            None => "another listener".to_string(),
        })
}

async fn tailscale(args: &[&str]) -> Result<Vec<u8>, ToolError> {
    match Tool::new("tailscale")
        .args(args)
        .timeout(TIMEOUT)
        .success()
        .await
    {
        Err(ToolError::NotFound { .. })
            if cfg!(target_os = "macos") && std::path::Path::new(MAC_APP_CLI).exists() =>
        {
            Tool::new(MAC_APP_CLI)
                .args(args)
                .timeout(TIMEOUT)
                .success()
                .await
        }
        other => other,
    }
}

/// The failure, with the remedy for the ones that have a known one.
fn explain(error: ToolError, command: &str) -> anyhow::Error {
    let stderr = error.stderr().to_ascii_lowercase();
    match &error {
        ToolError::NotFound { .. } => {
            anyhow::anyhow!("--tailscale needs the Tailscale CLI on PATH: {error}")
        }
        ToolError::TimedOut { .. } => anyhow::anyhow!(
            "`{command}` was still waiting after {}s, which it does when HTTPS or Serve is not \
             enabled for this tailnet; run it once by hand and follow the link it prints",
            TIMEOUT.as_secs()
        ),
        _ if stderr.contains("access denied") || stderr.contains("permission") => anyhow::anyhow!(
            "{error}\nTailscale refused this user; on Linux, `sudo tailscale set \
             --operator=$USER` lets it configure serve without root"
        ),
        _ => anyhow::anyhow!("{error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shape `tailscale serve status --json` printed on the machine this was written
    /// on: T3 Code on 443, a probe on 18787.
    fn config() -> Value {
        json!({
            "TCP": { "443": { "HTTPS": true }, "18787": { "HTTPS": true }, "2222": { "TCPForward": "127.0.0.1:22" } },
            "Web": {
                "box.ts.net:443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:3773" } } },
                "box.ts.net:18787": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:8787" } } },
            }
        })
    }

    #[test]
    fn a_port_is_held_by_its_proxy_target_or_by_nothing() {
        let config = config();
        assert_eq!(
            holder(&config, "box.ts.net", 443).as_deref(),
            Some("http://127.0.0.1:3773")
        );
        assert_eq!(
            holder(&config, "box.ts.net", 18787).as_deref(),
            Some("http://127.0.0.1:8787")
        );
        assert_eq!(holder(&config, "box.ts.net", 8787), None);
        assert_eq!(
            holder(&config, "box.ts.net", 2222).as_deref(),
            Some("a TCP forward to 127.0.0.1:22"),
            "a TCP forward is somebody's, and is never reclaimed"
        );
        assert_eq!(holder(&Value::Null, "box.ts.net", 443), None);
    }
}
