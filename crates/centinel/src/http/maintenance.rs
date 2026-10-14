//! Stoppable maintenance jobs sharing the store's collection lane.
use std::sync::Arc;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use centinel_core::{
    journal::{Holder, RunLock, Trigger},
    op::{Cancel, Ctx, Progress},
};
use serde_json::json;

/// Reserve before answering, hold through completion or cancellation, and never write a
/// collection attempt. Used by workspace operations that mutate collection or its indexes.
pub(super) fn spawn_maintenance<T, Fut>(
    ctx: &Arc<Ctx>,
    kind: &str,
    sources: Vec<String>,
    work: impl FnOnce(Arc<Ctx>, String, Progress, Cancel) -> Fut + Send + 'static,
) -> Response
where
    T: Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
{
    let holder = Holder {
        pid: std::process::id(),
        started_at: jiff::Timestamp::now().to_string(),
        trigger: Trigger::Manual,
        schedule: None,
        args: kind.to_string(),
        operation: Some(kind.to_string()),
        sources,
    };
    let lock = match RunLock::acquire(&ctx.store, &holder) {
        Ok(lock) => lock,
        Err(held) => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": held.to_string(), "running": held.0,
                })),
            )
                .into_response();
        }
    };
    let label = if holder.sources.is_empty() {
        "every source".into()
    } else {
        holder.sources.join(", ")
    };
    super::spawn_job(
        ctx,
        kind,
        label,
        move |ctx, id, progress, cancel| async move {
            // Some ops clear an index before their first item boundary.
            let result = match cancel.check() {
                Ok(()) => work(ctx, id, progress, cancel).await,
                Err(error) => Err(error),
            };
            // The finished event triggers a page refresh, which must see the lane released.
            drop(lock);
            result
        },
    )
}
