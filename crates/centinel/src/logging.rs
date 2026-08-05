//! What this binary says while it works, and the one place an op invocation is recorded.
//!
//! Three decisions live here.
//!
//! **Where it goes.** Always stderr. Under `centinel mcp` stdout carries JSON-RPC frames
//! and under an op it carries the report; a log line on either corrupts something a
//! program is parsing.
//!
//! **Whether it is on.** Decided by the surface, not by a flag. An op returns a report
//! and draws its own progress bars, so it stays quiet unless `--verbose` asks. A server
//! has neither — its stderr *is* its output — so `serve` and `mcp` log at info with no
//! flag. `RUST_LOG` beats both.
//!
//! **What one call looks like.** [`invoke`] is this crate's only call site of
//! [`op::OpDef::invoke`], so an invocation reads the same however it arrived and
//! `surface` is the only field that differs.

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use centinel_core::op::{Ctx, OpDef, Progress, ProgressEvent};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedReceiver;
use tracing_subscriber::EnvFilter;

/// The default filter for a surface that returns a report to a person and draws its own
/// progress. Silent, because both of those beat a log line — and because a stray line
/// lands in the middle of an `indicatif` bar and stays there.
const OP_DEFAULT: &str = "off";

/// The default filter for `serve` and `mcp`. A server that says nothing between
/// "listening" and its next crash cannot be tested, which is the whole reason this
/// module exists.
const SERVER_DEFAULT: &str = "centinel=info,centinel_core=info";

/// What `--verbose` selects, on every surface.
const VERBOSE_DEFAULT: &str = "centinel=debug,centinel_core=debug";

/// Installs the subscriber. Call once, before anything worth logging.
///
/// `surface` is the subcommand name rather than an enum, because the only distinction
/// that matters is "is this one of the two server commands" — and those are already
/// named once, in `SERVER_COMMANDS`.
pub fn install(surface: &str, verbose: bool, no_color: bool) {
    let default = match (surface, verbose) {
        (_, true) => VERBOSE_DEFAULT,
        ("serve" | "mcp", false) => SERVER_DEFAULT,
        _ => OP_DEFAULT,
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        // Decided by *stderr*, not by the `--color` machinery, which reads stdout: the
        // two go to different places and `centinel serve > /dev/null` is a normal thing
        // to type.
        .with_ansi(std::io::stderr().is_terminal() && !no_color)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| default.into()))
        .init();
}

/// Invokes an op and records the call: a line when it starts, a line when it ends, and
/// whatever it says in between.
///
/// `progress` is `None` from the surfaces with nowhere to draw it — a plain
/// `POST /ops/{name}` and every MCP tool call, both of which return exactly once. Those
/// get a sink that writes each event to the log instead. It is the difference between a
/// `run` over MCP being a silent hour and being watchable, and it costs the op nothing:
/// it still never learns who invoked it.
pub async fn invoke(
    surface: &'static str,
    def: &'static OpDef,
    ctx: Arc<Ctx>,
    args: Value,
    progress: Option<Progress>,
) -> Result<Value> {
    let (progress, drain) = match progress {
        Some(sink) => (sink, None),
        None => {
            let (sink, rx) = Progress::channel();
            (sink, Some(tokio::spawn(drain_to_log(def.name, rx))))
        }
    };

    tracing::info!(surface, op = def.name, args = %one_line(&args), "op started");
    let started = Instant::now();

    let result = (def.invoke)(ctx, args, progress).await;

    // The sink was dropped with the invocation, so the drain has already ended or is
    // about to; awaiting it just keeps the last progress line above the closing one.
    if let Some(drain) = drain {
        let _ = drain.await;
    }

    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(_) => tracing::info!(surface, op = def.name, elapsed_ms, "op finished"),
        Err(e) => {
            tracing::warn!(surface, op = def.name, elapsed_ms, error = %format!("{e:#}"), "op failed")
        }
    }
    result
}

/// Writes progress to the log for a surface that cannot draw it.
///
/// Split by what a [`ProgressEvent`] *is*: one with no `id` is a log line by
/// construction, one with an id is a bar redrawing. A crawl emits thousands of the
/// second kind, so they sit at debug and the info stream stays readable.
async fn drain_to_log(op: &'static str, mut rx: UnboundedReceiver<ProgressEvent>) {
    while let Some(event) = rx.recv().await {
        match (event.id.as_deref(), event.done, event.total) {
            (None, _, _) => tracing::info!(op, "{}", event.message),
            (Some(track), Some(done), Some(total)) => {
                tracing::debug!(op, track, done, total, "{}", event.message)
            }
            (Some(track), _, _) => tracing::debug!(op, track, "{}", event.message),
        }
    }
}

/// An argument set as one line short enough to sit in a log field.
///
/// `ingest` takes a list of URLs and `source add` takes a whole config; either runs to
/// kilobytes, and one line that wraps forty times hides the next one.
fn one_line(args: &Value) -> String {
    const LIMIT: usize = 300;

    let text = args.to_string();
    if text.len() <= LIMIT {
        return text;
    }
    // A URL list is mostly ASCII but a page title is not, so cut on a boundary rather
    // than on a byte and panicking in the logger.
    let cut = text
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|i| *i <= LIMIT)
        .last()
        .unwrap_or(0);
    format!("{}… ({} bytes)", &text[..cut], text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn short_arguments_are_logged_whole() {
        let args = json!({ "source": "tampa", "limit": 10 });
        assert_eq!(one_line(&args), args.to_string());
    }

    /// The failure this guards is a panic *inside the logger*, which would take down a
    /// request that was otherwise fine.
    #[test]
    fn a_long_argument_set_is_cut_on_a_character_boundary() {
        let args = json!({ "urls": vec!["https://exämple.gov/ä"; 100] });
        let line = one_line(&args);
        assert!(line.ends_with(&format!("({} bytes)", args.to_string().len())));
        assert!(line.len() < args.to_string().len());
    }
}
