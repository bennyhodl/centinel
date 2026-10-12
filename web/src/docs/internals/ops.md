# Ops

An op is an ordinary async function. Annotating it puts it on the CLI, in the MCP tool
list, and at an HTTP route — with **no central registration list to update**.

```rust
/// List every source — configured or collected — with resource counts and liveness.
#[op(group = "corpus")]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> { … }
```

```console
$ centinel list --max-problems 5              # CLI: flags and help from the same struct
$ curl -X POST localhost:8787/ops/list        # HTTP: JSON in, JSON out
{"jsonrpc":"2.0","method":"tools/list"}       # MCP: JSON Schema from the same struct
```

## The registry

```
  #[op] async fn search(&Ctx, SearchArgs) -> Result<SearchReport>
        │
        ├── augment_clap ─────────► CLI flags + help text
        ├── schema ───────────────► MCP tool JSON Schema / HTTP request body
        ├── invoke ───────────────► one type-erased call path for all three
        └── render ───────────────► the report, in a terminal's idiom (CLI only)
```

Registration is **link-time**, via `inventory`. There is nowhere to forget to add an op.
The binary names no individual op; it iterates the registry — including for `--help`,
which is why an op cannot exist and be invisible.

**Why a proc macro** rather than build-time codegen or a runtime registry: codegen puts
generated source in the tree and makes the definition site not the source of truth; a
runtime registry needs an explicit `register(…)` call per op — exactly the central list
this avoids, and exactly the thing people forget. Accepted cost: proc macros degrade error
messages, mitigated by keeping the expansion thin.

## Ops are thin

Argument validation, a call into the store or into acquisition against a `Source`, and a
serializable result. Behaviour that deserves tests belongs in the library, not in an op
body.

No op knows a site from a channel. `discover` and `collect` name what happens, not how.
Which adapter you get is decided once, from the `[[source]]` block.

## Three axes on every op

**Group** — `pipeline`, `stage`, `corpus` or `host`. It decides only the heading the op
lists under in `centinel --help`. Sixteen verbs in one alphabetical column make `collect`,
`embed` and `doctor` look like peer choices, when the first two are steps of what `run`
does for you and the third is a health check.

**Reach** — who may cause it to run:

| `Reach` | CLI | Scheduler | HTTP | MCP |
|---|:--:|:--:|:--:|:--:|
| `Public` | ● | — | ● | ● |
| `Operator` | ● | ● | ✕ | ✕ |
| `Host` | ● | ✕ | ✕ | ✕ |

Two independent booleans would describe four states, and only three exist. The fourth —
"the scheduler may fire it **and** so may any HTTP caller" — is the exact defect this
enum exists to prevent, and a pair of booleans leaves it one typo away.

There is a registry-wide invariant rather than a list of names: **every op in `pipeline`
or `stage` must have `Reach::Operator`.** A test over the whole registry covers the op
somebody adds next year, not the ones somebody remembered.

Both enforcement points matter: the listings filter non-`Public` ops out, *and* the HTTP
handler refuses them on call. Hiding alone is not access control.

**Long-running** — whether the op emits progress.

All three live on the op rather than in the CLI crate, for the same reason registration
does: there is nowhere to forget them.

## Reports are rendered, not printed

A report is the right shape for HTTP and MCP — a model reads JSON better than it reads a
table — and the wrong shape for a person, who gets forty lines of quoted keys where four
lines would do.

```console
$ centinel list                    # a terminal → prose
$ centinel list | jq '.sources'    # a pipe → JSON, exactly as before
$ centinel list --json             # force JSON on a terminal
$ centinel search x --pretty | less -R    # force prose into a pager
```

The destination decides the default. `--json` / `--pretty` override the format and
`--color=auto|always|never` overrides the colour, independently. `NO_COLOR` is honoured
and loses only to an explicit `--color always`.

Rendering reads **the same erased JSON `invoke` produced**, so a terminal can never be
shown a field HTTP would not return — and a report that `skip_serializing_if` hides from
the wire is equally invisible here. That round-trip means every report type must
deserialize from its own serialized form, which is a property any Rust consumer of the
HTTP API needs anyway.

Each report implements `Render` beside its own definition, and there is **no structural
fallback** — a new op will not compile until its report says how it reads. That is the
opposite of a central list: forgetting is impossible because the compiler asks at the
definition site, in the one place that knows what the numbers mean.

## Long-running operations

The hardest case. Ops emit progress one way and never learn who called them.

| Surface | Rendering |
|---|---|
| CLI | progress bars on stderr when stderr is a terminal, plain lines when it is a pipe — so stdout carries only the report and stays a clean JSON stream whenever it is piped |
| HTTP | `POST /ops/{name}/stream` → SSE progress frames, then a terminal `result` or `error` |
| MCP | waits and returns once — base MCP has no streaming channel for tool results |

A `ProgressEvent` carries an optional **`id`** and a **`unit`**. Events sharing an `id` are
one unit of work, so a renderer can keep a bar per file plus an aggregate beside it rather
than one bar whose meaning shifts underneath the operator. `unit: bytes` is what turns
`312000000/613527539` into `297 MiB / 585 MiB at 18.4 MiB/s`.

Both are presentation hints. The op emits them and never learns whether anything drew a
bar.

`/stream` holds the connection open rather than returning a job id — honest for the spine,
and the durable job store belongs with scheduling.

## Adding one

1. Write the async function in `crates/centinel-core/src/ops/`.
2. Annotate it with `#[op(...)]`, giving it a group and a reach.
3. Give its args struct `clap::Args`, `Serialize`, `Deserialize`, `JsonSchema`.
4. Implement `Render` for its report, beside the report.

There is no step 5. It is now a CLI subcommand, an MCP tool and an HTTP route.

Next: [Commands](../reference/commands.md).
