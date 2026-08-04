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

/// How to read [`ProgressEvent::done`] and `total`.
///
/// Presentation, not semantics — the op says what it is counting, and each surface
/// decides how to render it. `Bytes` is what turns `312000000/613527539` into
/// `297 MiB / 585 MiB at 18.4 MiB/s`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Bare items: URLs fetched, documents extracted.
    #[default]
    Count,
    Bytes,
}

impl Unit {
    /// So the default stays off the wire and old consumers see the old shape.
    fn is_count(&self) -> bool {
        matches!(self, Self::Count)
    }
}

/// The reserved `id` for an aggregate track — "this whole operation", as opposed to the
/// individual unit of work in flight.
///
/// Reserved rather than conventional: a surface has to be able to tell the summary line
/// from the item lines to lay them out, and an op that produced a track called `total`
/// for its own reasons would otherwise silently overwrite the summary. The leading
/// underscores keep it out of any namespace a real work item would use.
pub const TOTAL_TRACK: &str = "__total__";

/// A progress report from a long-running op.
///
/// This is the shape all three surfaces render: a progress bar on the CLI, an SSE frame
/// over HTTP, a notification over MCP.
///
/// `id` is what makes **concurrent or sequential multi-part work** legible: events
/// sharing an id are the same unit of work, so a renderer can keep one bar per file and
/// an aggregate bar beside it, rather than one bar whose meaning changes underneath the
/// operator. An event with no `id` is a log line, not a bar.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    /// Which unit of work this reports on. `None` means "a message, not a bar".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Unit::is_count")]
    pub unit: Unit,
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
            ..Default::default()
        });
    }

    /// Convenience for counted work with a single implicit track.
    pub fn step(&self, message: impl Into<String>, done: u64, total: u64) {
        self.send(ProgressEvent {
            message: message.into(),
            done: Some(done),
            total: Some(total),
            ..Default::default()
        });
    }

    /// Reports on a *named* unit of work — one bar per `id`.
    ///
    /// Reaching `done == total` is how a surface knows a track finished, so an op that
    /// completes one should say so explicitly rather than simply stopping.
    pub fn track(
        &self,
        id: impl Into<String>,
        message: impl Into<String>,
        done: u64,
        total: u64,
        unit: Unit,
    ) {
        self.send(ProgressEvent {
            message: message.into(),
            done: Some(done),
            total: Some(total),
            id: Some(id.into()),
            unit,
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

/// Renders an op's result for a person at a terminal.
///
/// Takes the same erased JSON [`InvokeFn`] produced, so the CLI needs no second call path
/// and cannot render something the other two surfaces did not receive. The macro puts the
/// concrete type back on before handing off to [`crate::render::Render`] — which is what
/// lets a renderer read `report.failures[0].detail` instead of indexing a `Value`.
pub type RenderFn = fn(&serde_json::Value, &mut crate::render::Painter<'_>) -> anyhow::Result<()>;

/// Which heading an op lists under in `centinel --help`.
///
/// Presentation only — every surface still reaches every op, and nothing here changes
/// what an op *is*. It exists because a flat alphabetical list of sixteen verbs makes
/// `collect`, `embed` and `doctor` look like peer choices when the first two are steps
/// of a pipeline `run` performs for you and the third is a health check.
///
/// Declared at the op, so the grouping is not a list in the CLI crate to forget to
/// update — the same reason registration is link-time (SPEC §8, ticket #9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// What you run. `run` and the config that feeds it.
    Pipeline,
    /// One stage of what `run` does, for when you want just that stage.
    Stage,
    /// Questions asked of what has already been collected.
    Corpus,
    /// The machine, not the corpus.
    Host,
}

impl Group {
    /// Reading order for `--help`: what to type first, then its parts, then the corpus,
    /// then the machine.
    pub const ORDER: [Group; 4] = [Self::Pipeline, Self::Stage, Self::Corpus, Self::Host];

    /// The heading text. Plural where the group holds interchangeable members.
    pub fn heading(&self) -> &'static str {
        match self {
            Self::Pipeline => "Pipeline",
            Self::Stage => "Stages",
            Self::Corpus => "Corpus",
            Self::Host => "Host",
        }
    }
}

/// Every op in a group, in registry (alphabetical) order.
pub fn in_group(group: Group) -> Vec<&'static OpDef> {
    all().into_iter().filter(|o| o.group == group).collect()
}

/// One registered operation.
///
/// Built by [`centinel_macros::op`] and submitted to the `inventory` registry. Nothing
/// constructs this by hand.
pub struct OpDef {
    /// Kebab-case, unique. The CLI subcommand, the MCP tool name, the HTTP path segment.
    pub name: &'static str,
    /// One line. Becomes clap's `about` and the MCP tool description.
    pub about: &'static str,
    /// Which `--help` heading this op lists under. CLI presentation only.
    pub group: Group,
    /// Whether this op reports progress worth streaming. Surfaces use it to decide
    /// between a plain response and a streamed one.
    pub long_running: bool,
    /// Whether to expose this op as an MCP tool.
    ///
    /// Not every library function should be one — ticket #9's "a model does not need
    /// forty tools". Exposure is **opt-out**: uniform by default, curated deliberately.
    pub mcp: bool,
    /// This op acts on the machine it runs on, so remote invocation is meaningless or
    /// dangerous. Excluded from **both** MCP and HTTP; reachable only from the CLI.
    ///
    /// `open` is the motivating case: it launches a GUI application on the host and
    /// accepts a command template. Exposed remotely that is arbitrary command execution
    /// against a server that (SPEC §8) has no authentication.
    pub local_only: bool,
    /// Adds this op's arguments to a `clap::Command`.
    pub augment_clap: fn(clap::Command) -> clap::Command,
    /// Extracts parsed CLI arguments as the same JSON the other surfaces send.
    pub args_from_matches: fn(&clap::ArgMatches) -> anyhow::Result<serde_json::Value>,
    /// JSON Schema for the argument type — MCP `inputSchema`, HTTP request body.
    pub schema: fn() -> serde_json::Value,
    pub invoke: InvokeFn,
    /// How this op's report reads on a terminal. CLI-only; HTTP and MCP want the JSON.
    pub render: RenderFn,
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
    all()
        .into_iter()
        .filter(|o| o.mcp && !o.local_only)
        .collect()
}

/// Ops reachable over HTTP.
pub fn remote_ops() -> Vec<&'static OpDef> {
    all().into_iter().filter(|o| !o.local_only).collect()
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
    pub use super::{Ctx, Group, OpDef, Progress};
    pub use crate::render::{Painter, Render};

    /// Builds the `render` body: JSON → the concrete report → its own prose.
    ///
    /// The round-trip through `Value` is deliberate. Rendering the *serialized* form is
    /// what guarantees a terminal and an HTTP caller are looking at the same report — a
    /// field that `skip_serializing_if` hides from the wire is equally invisible here,
    /// rather than appearing on one surface only.
    pub fn render_as<O>(
        value: &serde_json::Value,
        p: &mut Painter<'_>,
    ) -> anyhow::Result<()>
    where
        O: serde::de::DeserializeOwned + Render,
    {
        let report: O = serde_json::from_value(value.clone())?;
        report.render(p)?;
        Ok(())
    }

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

    #[tokio::test]
    async fn tracks_are_distinguished_by_id() {
        let (p, mut rx) = Progress::channel();
        p.track("a", "file a", 1, 10, Unit::Bytes);
        p.track("b", "file b", 5, 10, Unit::Bytes);
        drop(p);

        let a = rx.recv().await.unwrap();
        let b = rx.recv().await.unwrap();
        assert_eq!(a.id.as_deref(), Some("a"));
        assert_eq!(b.id.as_deref(), Some("b"));
        assert_eq!(a.unit, Unit::Bytes);
    }

    /// The added fields must not change what an existing counted op puts on the wire —
    /// SSE consumers of `/ops/{name}/stream` predate them.
    #[test]
    fn counted_events_serialize_exactly_as_before() {
        let ev = ProgressEvent {
            message: "collected".into(),
            done: Some(3),
            total: Some(9),
            ..Default::default()
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"message": "collected", "done": 3, "total": 9})
        );
    }

    #[test]
    fn byte_tracks_carry_their_unit_and_id() {
        let ev = ProgressEvent {
            message: "model.onnx".into(),
            done: Some(1),
            total: Some(2),
            id: Some("m/model.onnx".into()),
            unit: Unit::Bytes,
        };
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["unit"], "bytes");
        assert_eq!(json["id"], "m/model.onnx");
    }

    /// It must not collide with anything an op would name a work item, or that item
    /// would silently overwrite the aggregate.
    #[test]
    fn the_aggregate_track_id_is_reserved() {
        assert!(TOTAL_TRACK.starts_with("__"));
        assert!(!TOTAL_TRACK.contains('/'));
    }

    #[test]
    fn an_event_without_the_new_fields_still_deserializes() {
        let ev: ProgressEvent =
            serde_json::from_str(r#"{"message":"old","done":1,"total":2}"#).unwrap();
        assert!(ev.id.is_none());
        assert_eq!(ev.unit, Unit::Count);
    }
}

#[cfg(test)]
mod render_path_tests {
    use super::*;
    use crate::render::Painter;
    use crate::store::Store;

    /// Renders an op's real output through the erased [`RenderFn`], exactly as the CLI does.
    async fn render_op(name: &str, args: serde_json::Value) -> String {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        let ctx = Arc::new(Ctx::new(store));

        let def = find(name).unwrap_or_else(|| panic!("op `{name}` is not registered"));
        let value = (def.invoke)(ctx, args, Progress::none())
            .await
            .expect("op failed");

        let mut buf: Vec<u8> = Vec::new();
        {
            let mut p = Painter::new(&mut buf, false, 100);
            (def.render)(&value, &mut p)
                .unwrap_or_else(|e| panic!("rendering `{name}` failed: {e}"));
        }
        String::from_utf8(buf).unwrap()
    }

    /// The end-to-end guard on the erased render path.
    ///
    /// `render` re-deserializes the JSON that `invoke` produced, so any report field with
    /// `skip_serializing_if` and no matching `default` makes the CLI fail at the last
    /// step — after the work is done — on a report the HTTP surface returns happily.
    /// That is not a rendering bug; it means the type cannot be parsed from its own output,
    /// and any Rust consumer of the HTTP API hits it too.
    #[tokio::test]
    async fn a_report_can_be_rendered_from_its_own_serialized_form() {
        let out = render_op("list", serde_json::json!({"max_problems": 20})).await;
        assert!(out.contains("No sources"), "unexpected empty-store render: {out:?}");

        // `doctor` is the one that actually broke: `GateStatus::missing` is skipped when
        // empty, so a machine with every model installed produced JSON that would not
        // deserialize back.
        let out = render_op("doctor", serde_json::json!({"skip_blob_count": true})).await;
        assert!(out.contains("binaries") && out.contains("gates"), "{out:?}");
    }

    /// Rendering must not emit escape codes when colour is off — the property that keeps
    /// `--pretty > file` and `NO_COLOR` honest.
    #[tokio::test]
    async fn rendering_without_colour_stays_plain_text() {
        let out = render_op("doctor", serde_json::json!({"skip_blob_count": true})).await;
        assert!(!out.contains('\x1b'), "escape codes leaked with colour off");
    }

    /// Every op must be renderable, not just the ones with a test above. The macro makes
    /// this a compile-time guarantee; this asserts the registry actually carries it.
    #[test]
    fn every_op_carries_a_renderer() {
        for def in all() {
            let rendered = {
                let mut buf: Vec<u8> = Vec::new();
                let mut p = Painter::new(&mut buf, false, 100);
                // A null value cannot deserialize into any report, so this must be an
                // error rather than a panic — a renderer that unwraps would take the CLI
                // down instead of reporting.
                (def.render)(&serde_json::Value::Null, &mut p).is_err()
            };
            assert!(rendered, "op `{}` rendered from null", def.name);
        }
    }
}
