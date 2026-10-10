//! What this binary says while it works, and the one place an op invocation is recorded.
//!
//! Three decisions live here.
//!
//! **Where it goes.** Always stderr. Under `centinel mcp` stdout carries JSON-RPC frames
//! and under an op it carries the report; a log line on either corrupts something a
//! program is parsing. A command's *result* — search hits, `read` text, `--json` — is
//! stdout's and never logged; what the command did on the way there is stderr's and
//! never printed.
//!
//! **How much.** Info on every surface, with no flag. Info is what a person watching
//! wants to see — this source, this page, this batch, done — and it is the same lines
//! whether they are watching a terminal, a scheduled run's journal, or an MCP client's
//! captured stderr. `-v` adds the internals (which calls were made, how big, how long,
//! which device) and `-q` keeps only warnings. `RUST_LOG` beats all three, because it is
//! the only way to name a single module or a vendored library.
//!
//! **What one call looks like.** [`invoke`] is this crate's only call site of
//! [`OpDef::invoke`], so an invocation reads the same however it arrived and
//! `surface` is the only field that differs. It opens a span the op's every line sits
//! under, it is where a long-running op becomes a job in [`centinel_core::jobs`], and
//! it is where an op with nowhere else to report is handed a sink that writes to the
//! log — for the same reason each time: one place, every surface.

use std::io::IsTerminal;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use centinel_core::op::{Cancel, Ctx, OpDef, Progress};
use serde_json::Value;
use tracing::Instrument;
use tracing_subscriber::EnvFilter;

/// The filter with no flag: everything a person watching wants, nothing they would
/// scroll past.
const DEFAULT: &str = "centinel=info,centinel_core=info";

/// What `-v` selects. This crate and the library, not the dependencies: `llama.cpp`
/// alone writes hundreds of lines per model load, and a debug run that drowned the batch
/// it was asked about would be no use. `RUST_LOG` names a library when one is wanted.
const VERBOSE: &str = "centinel=debug,centinel_core=debug";

/// What `-q` selects: only what went wrong.
const QUIET: &str = "centinel=warn,centinel_core=warn";

/// Installs the subscriber. Call once, before anything worth logging.
///
/// `verbose` and `quiet` are the two flags, already known by clap to conflict, so the
/// order of the match is never exercised.
pub fn install(verbose: bool, quiet: bool, no_color: bool) {
    let default = match (verbose, quiet) {
        (true, _) => VERBOSE,
        (_, true) => QUIET,
        _ => DEFAULT,
    };

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        // Decided by *stderr*, not by the `--color` machinery, which reads stdout: the
        // two go to different places and `centinel serve > /dev/null` is a normal thing
        // to type.
        .with_ansi(std::io::stderr().is_terminal() && !no_color)
        // The module path is noise to the person these lines are for; the span the op
        // opens says which op and which surface, which is the context that matters.
        .with_target(false)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| default.into()))
        .init();
}

/// Invokes an op and records the call: a line when it starts, a line when it ends, and
/// whatever it says in between.
///
/// `progress` is `None` from every surface with nowhere to draw it — the CLI, a plain
/// `POST /ops/{name}`, every MCP tool call, a scheduled run. Those get
/// [`Progress::logged`], which writes each event to the log at the level
/// `centinel_core::op` decides. Only `/ops/{name}/stream` passes a sink of its own,
/// because it has a caller waiting for the frames.
pub async fn invoke(
    surface: &'static str,
    def: &'static OpDef,
    ctx: Arc<Ctx>,
    args: Value,
    progress: Option<Progress>,
) -> Result<Value> {
    invoke_cancellable(surface, def, ctx, args, progress, Cancel::none()).await
}

/// [`invoke`], for a caller that may need to stop the work.
///
/// Only the scheduler passes a live token today: the CLI's interruption is the process
/// dying, and an HTTP or MCP caller hanging up cannot ask for a crawl to stop — it never
/// had the authority to start one (SPEC scheduling §1.1).
pub async fn invoke_cancellable(
    surface: &'static str,
    def: &'static OpDef,
    ctx: Arc<Ctx>,
    args: Value,
    progress: Option<Progress>,
    cancel: Cancel,
) -> Result<Value> {
    // Every line the op writes carries which op and from where. The name is the span's
    // own field rather than its name so `op{run cli}` reads as one thing across a run
    // that nests stages under it.
    let span = tracing::info_span!("op", name = def.name, surface);
    async move {
        let progress = progress.unwrap_or_else(Progress::logged);

        tracing::info!(args = %one_line(&args), "started");
        let started = Instant::now();

        // A long-running op is worth watching from elsewhere; a `list` is over before
        // anyone could look.
        let job = def.long_running.then(|| ctx.jobs.start(def.name, surface));
        let progress = match &job {
            Some(job) => job.watch(progress),
            None => progress,
        };

        let result = (def.invoke)(ctx, args, progress, cancel).await;
        if let Some(job) = job {
            job.finish(&result);
        }

        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(_) => tracing::info!(elapsed_ms, "finished"),
            Err(e) => tracing::warn!(elapsed_ms, error = %format!("{e:#}"), "failed"),
        }
        result
    }
    .instrument(span)
    .await
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
