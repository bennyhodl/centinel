//! MCP, derived from the op registry.
//!
//! SPEC §1 locks "MCP is derived from the library API, not hand-written". This module
//! is the proof: it contains no tool definitions. `tools/list` is
//! [`centinel_core::op::mcp_tools`] rendered as JSON Schema, and `tools/call` is a
//! registry lookup plus [`centinel_core::op::OpDef::invoke`].
//!
//! The transport is line-delimited JSON-RPC 2.0 over stdio. Hand-rolled rather than
//! taken from an SDK because the surface actually used here is four methods, and a
//! dependency would obscure exactly the derivation this module exists to demonstrate.
//!
//! **stdout is the protocol channel.** Anything written there that is not a JSON-RPC
//! frame corrupts the stream — which is why logging is pinned to stderr in `main`.

use std::sync::Arc;

use anyhow::Result;
use centinel_core::op::{self, Ctx, Progress};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The MCP revision this server implements.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves MCP over stdio until stdin closes.
pub async fn serve(ctx: Arc<Ctx>) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&ctx, req).await,
            Err(e) => Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            )),
        };

        // Notifications produce no response — replying to one is a protocol violation.
        if let Some(resp) = response {
            let mut bytes = serde_json::to_vec(&resp)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// Dispatches one JSON-RPC request. `None` means "this was a notification".
///
/// Shared with the HTTP surface so MCP-over-stdio and MCP-over-HTTP cannot drift.
pub async fn handle(ctx: &Arc<Ctx>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = req.get("params").cloned().unwrap_or(Value::Null);

    // No `id` means a notification: act, but never reply.
    let id = id?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": {
                "name": "centinel",
                "version": env!("CARGO_PKG_VERSION"),
            },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => return Some(call_tool(ctx, id, params).await),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => error_response(id, code, &message),
    })
}

/// The tool list, rendered straight from the registry.
fn tool_list() -> Vec<Value> {
    op::mcp_tools()
        .into_iter()
        .map(|def| {
            let mut schema = (def.schema)();
            // `$schema` is meta-information about the schema dialect, not part of the
            // input contract; some clients reject it inside `inputSchema`.
            if let Some(obj) = schema.as_object_mut() {
                obj.remove("$schema");
                obj.remove("title");
            }
            json!({
                "name": def.name,
                "description": def.about,
                "inputSchema": schema,
            })
        })
        .collect()
}

async fn call_tool(ctx: &Arc<Ctx>, id: Value, params: Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let Some(def) = op::find(name) else {
        return error_response(id, -32602, &format!("unknown tool: {name}"));
    };
    if !def.mcp {
        return error_response(
            id,
            -32602,
            &format!("tool `{name}` is not exposed over MCP"),
        );
    }

    // Progress is dropped here. Base MCP has no streaming channel for tool results, so
    // a long op simply takes a while and returns once — the honest mapping, and the
    // reason `long_running` exists as a hint the *other* surfaces act on.
    match (def.invoke)(Arc::clone(ctx), arguments, Progress::none()).await {
        Ok(value) => {
            let text = serde_json::to_string_pretty(&value)
                .unwrap_or_else(|e| format!("<unserializable result: {e}>"));
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "structuredContent": value,
                    "isError": false,
                },
            })
        }
        // Tool *execution* failures are results with `isError`, not protocol errors —
        // the model is meant to see them and adapt, not have the call rejected.
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": format!("{e:#}") }],
                "isError": true,
            },
        }),
    }
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use centinel_core::store::Store;

    async fn ctx() -> (tempfile::TempDir, Arc<Ctx>) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        (dir, Arc::new(Ctx::new(store)))
    }

    #[tokio::test]
    async fn initialize_advertises_tools() {
        let (_d, ctx) = ctx().await;
        let resp = handle(&ctx, json!({"jsonrpc":"2.0","id":1,"method":"initialize"}))
            .await
            .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn notifications_get_no_reply() {
        let (_d, ctx) = ctx().await;
        let resp = handle(
            &ctx,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        assert!(
            resp.is_none(),
            "replying to a notification breaks the protocol"
        );
    }

    #[tokio::test]
    async fn tools_list_matches_the_registry() {
        let (_d, ctx) = ctx().await;
        let resp = handle(&ctx, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .await
            .unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();

        assert_eq!(tools.len(), op::mcp_tools().len());
        for tool in tools {
            assert!(tool["inputSchema"].is_object());
            assert!(
                tool["inputSchema"].get("$schema").is_none(),
                "dialect metadata must not leak into inputSchema"
            );
            assert!(!tool["description"].as_str().unwrap().is_empty());
        }
        assert!(tools.iter().any(|t| t["name"] == "doctor"));
    }

    #[tokio::test]
    async fn tools_call_runs_an_op() {
        let (_d, ctx) = ctx().await;
        let resp = handle(
            &ctx,
            json!({
                "jsonrpc":"2.0","id":3,"method":"tools/call",
                "params": {"name":"list","arguments":{}}
            }),
        )
        .await
        .unwrap();

        assert_eq!(resp["result"]["isError"], false);
        assert!(resp["result"]["structuredContent"]["sources"].is_array());
    }

    #[tokio::test]
    async fn bad_arguments_surface_as_tool_errors_not_protocol_errors() {
        let (_d, ctx) = ctx().await;
        let resp = handle(
            &ctx,
            json!({
                "jsonrpc":"2.0","id":4,"method":"tools/call",
                "params": {"name":"ingest","arguments":{"source":"Bad Source!","urls":[]}}
            }),
        )
        .await
        .unwrap();

        assert!(
            resp.get("error").is_none(),
            "should be a result, not an error"
        );
        assert_eq!(resp["result"]["isError"], true);
    }

    #[tokio::test]
    async fn unknown_method_is_a_protocol_error() {
        let (_d, ctx) = ctx().await;
        let resp = handle(&ctx, json!({"jsonrpc":"2.0","id":5,"method":"nope"}))
            .await
            .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }
}
