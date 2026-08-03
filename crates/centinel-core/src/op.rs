//! The op registry — the mechanism behind SPEC §1's "MCP is derived, not hand-written".
//!
//! One annotated function yields three consumers. The registry is what they share:
//!
//! ```text
//!   #[op] async fn search(&Ctx, SearchArgs) -> Result<SearchOut>
//!         │
//!         ├── augment_clap ─────────► CLI flags + help text
//!         ├── schema ───────────────► MCP tool JSON Schema / HTTP request body
//!         └── invoke ───────────────► one type-erased call path for all three
//! ```
//!
//! Registration is link-time via `inventory`, so adding an op requires **no edit to a
//! central list**. That is the property that makes the "one definition" claim true
//! rather than aspirational: there is nowhere to forget to add it.
//!
//! ## Where the mapping is deliberately *not* mechanical
//!
//! Ticket #9 asks how much per-consumer override is allowed before "one definition" is
//! a fiction. The answer taken here: **presence is uniform, prose is not.** Every op is
//! reachable from all three surfaces unless [`OpDef::mcp`] is false, but each surface
//! renders the same schema in its own idiom — clap gets flags and short help, MCP gets
//! the full description written for a model to read.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// Everything an op is handed besides its own arguments.
///
/// Deliberately small. An op that needs more should take it as an argument, so the
/// dependency shows up in the schema — and therefore in the CLI help and the MCP tool
/// description — instead of hiding in ambient state.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub store: Store,
}

impl Ctx {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

/// A progress report from a long-running op.
///
/// This is the shape all three surfaces render: a progress bar on the CLI, an SSE frame
/// over HTTP, a notification over MCP.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// The sink an op reports progress into.
///
/// A crawl is not request/response — ticket #9 flags this as the hardest case. The
/// resolution: ops always *emit* progress the same way, and each surface decides what
/// to do with it. An op never knows which consumer invoked it.
///
/// Detached sinks are free — [`Progress::none`] drops events, so an op needs no
/// branching for the "nobody is listening" case.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    tx: Option<tokio::sync::mpsc::UnboundedSender<ProgressEvent>>,
}

impl Progress {
    /// A sink that discards events.
    pub fn none() -> Self {
        Self { tx: None }
    }

    /// A sink plus the receiver a surface drains.
    pub fn channel() -> (Self, tokio::sync::mpsc::UnboundedReceiver<ProgressEvent>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { tx: Some(tx) }, rx)
    }

    /// Reports progress. Never blocks, never fails — a dropped receiver is not an
    /// op's problem, and must not turn into an error in the op's own result.
    pub fn send(&self, event: ProgressEvent) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }

    /// Convenience for the common "just a message" case.
    pub fn say(&self, message: impl Into<String>) {
        self.send(ProgressEvent {
            message: message.into(),
            done: None,
            total: None,
        });
    }

    /// Convenience for counted work.
    pub fn step(&self, message: impl Into<String>, done: u64, total: u64) {
        self.send(ProgressEvent {
            message: message.into(),
            done: Some(done),
            total: Some(total),
        });
    }
}

/// A type-erased op invocation: JSON in, JSON out.
///
/// Erasure is what lets one registry serve three surfaces. The macro generates the
/// concrete deserialize → call → serialize wrapper, so the erasure costs the op author
/// nothing and stays invisible at the definition site.
pub type InvokeFn = fn(
    Arc<Ctx>,
    serde_json::Value,
    Progress,
) -> BoxFuture<'static, anyhow::Result<serde_json::Value>>;

/// One registered operation.
///
/// Built by [`centinel_macros::op`] and submitted to the `inventory` registry. Nothing
/// constructs this by hand.
pub struct OpDef {
    /// Kebab-case, unique. The CLI subcommand, the MCP tool name, the HTTP path segment.
    pub name: &'static str,
    /// One line. Becomes clap's `about` and the MCP tool description.
    pub about: &'static str,
    /// Whether this op reports progress worth streaming. Surfaces use it to decide
    /// between a plain response and a streamed one.
    pub long_running: bool,
    /// Whether to expose this op as an MCP tool.
    ///
    /// Not every library function should be one — ticket #9's "a model does not need
    /// forty tools". Exposure is **opt-out**: uniform by default, curated deliberately.
    pub mcp: bool,
    /// Adds this op's arguments to a `clap::Command`.
    pub augment_clap: fn(clap::Command) -> clap::Command,
    /// Extracts parsed CLI arguments as the same JSON the other surfaces send.
    pub args_from_matches: fn(&clap::ArgMatches) -> anyhow::Result<serde_json::Value>,
    /// JSON Schema for the argument type — MCP `inputSchema`, HTTP request body.
    pub schema: fn() -> serde_json::Value,
    pub invoke: InvokeFn,
}

inventory::collect!(OpDef);

/// Every registered op, sorted by name for stable output across all three surfaces.
pub fn all() -> Vec<&'static OpDef> {
    let mut v: Vec<&'static OpDef> = inventory::iter::<OpDef>.into_iter().collect();
    v.sort_by_key(|o| o.name);
    v
}

/// Ops exposed as MCP tools.
pub fn mcp_tools() -> Vec<&'static OpDef> {
    all().into_iter().filter(|o| o.mcp).collect()
}

/// Looks up an op by name.
pub fn find(name: &str) -> Option<&'static OpDef> {
    all().into_iter().find(|o| o.name == name)
}

/// Re-exports the macro needs at expansion sites. Not a stable public API.
#[doc(hidden)]
pub mod __private {
    pub use anyhow;
    pub use clap;
    pub use futures;
    pub use inventory;
    pub use schemars;
    pub use serde_json;

    /// Re-exported so expansion sites can name these without importing them.
    pub use super::{Ctx, OpDef, Progress};

    /// Builds the `args_from_matches` body: clap → struct → JSON.
    ///
    /// Routing CLI arguments through the same JSON the other two surfaces send is what
    /// keeps them honest — a divergence would show up as a deserialize failure rather
    /// than as quietly different behaviour.
    pub fn args_to_json<A>(m: &clap::ArgMatches) -> anyhow::Result<serde_json::Value>
    where
        A: clap::FromArgMatches + serde::Serialize,
    {
        let parsed = A::from_arg_matches(m)?;
        Ok(serde_json::to_value(parsed)?)
    }

    pub fn schema_of<A: schemars::JsonSchema>() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(A))
            .expect("a JsonSchema always serializes to JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn detached_progress_is_a_no_op() {
        let p = Progress::none();
        p.say("nobody is listening");
        p.step("still fine", 1, 2);
    }

    #[tokio::test]
    async fn channel_progress_delivers_in_order() {
        let (p, mut rx) = Progress::channel();
        p.say("first");
        p.step("second", 1, 3);
        drop(p);

        let a = rx.recv().await.unwrap();
        let b = rx.recv().await.unwrap();
        assert_eq!(a.message, "first");
        assert_eq!(b.done, Some(1));
        assert_eq!(b.total, Some(3));
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn dropped_receiver_does_not_fail_the_op() {
        let (p, rx) = Progress::channel();
        drop(rx);
        p.say("into the void");
    }
}
