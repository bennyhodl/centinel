# From an agent

Centinel is a library, a CLI, an HTTP server and an MCP endpoint. Every verb is reachable
from all four, because all four are generated from the same function definition.

Agents sit **on top**. They are clients of the record, never its author. What gets
collected does not depend on what any model happened to think that day.

## MCP over stdio

```bash
centinel mcp
```

Point any MCP client at that command. Every op appears in `tools/list` with a JSON Schema
derived from its argument struct — the same struct that produces the CLI's flags and help
text, so the two can never drift.

MCP tool calls **wait and return once**. Base MCP has no streaming channel for tool
results, so a long-running op holds the call open. For a multi-hour crawl, prefer the HTTP
streaming route or the CLI.

## HTTP

```bash
centinel serve --bind 127.0.0.1:8787
```

| Route | Purpose |
|---|---|
| `GET /health` | liveness |
| `GET /ops` | the registry, with JSON Schema per op |
| `POST /ops/{name}` | invoke — JSON in, JSON out |
| `POST /ops/{name}/stream` | invoke with SSE progress frames, then a terminal `result` or `error` |
| `POST /mcp` | MCP JSON-RPC over HTTP, sharing the stdio handler |

```bash
curl -X POST localhost:8787/ops/search \
  -H 'content-type: application/json' \
  -d '{"query":"lobbyist meeting log","limit":5}'
```

Op failures are **400, not 500**. Nearly every failure reachable here is a bad argument or
an unreachable upstream, which is caller-actionable.

**There is no access control**, which is why the default bind is loopback. Binding to a
non-loopback address logs a warning rather than silently exposing the store. A scheme for
this is deliberately unspecified — inventing one here would foreclose the decision.

`serve` also fires the configured schedules. `--no-schedule` serves the read API without
them, which is what a machine that serves a corpus somebody else collects wants.

## Why serve rather than shell out

Both `serve` and `mcp` load the embedder and the reranker once and keep them. A short CLI
invocation pays the model load on every query — 11 seconds measured with only the 0.6B
reranker resident, more once the 4B embedder is also being built. An agent asking a
sequence of questions should hold one process open.

## Reach

Every op declares a **reach** — who may cause it to run — and the remote surfaces honour
it:

| `Reach` | CLI | Scheduler | HTTP | MCP | Ops |
|---|:--:|:--:|:--:|:--:|---|
| `Public` | ● | — | ● | ● | `search`, `read`, `list`, `doctor`, `schedules`, `history` |
| `Operator` | ● | ● | ✕ | ✕ | `run` and every stage, `ingest`, `source`, `schedule` |
| `Host` | ● | ✕ | ✕ | ✕ | `open`, `models` |

`Operator` ops change the corpus, so letting a remote caller add a source is letting it
choose the corpus one step earlier. `Host` ops act on the machine — `open` launches a
configured command, `models` pulls gigabytes — and not even the scheduler may fire them,
because a multi-gigabyte download must never ambush a 3am run.

A non-`Public` op is **invisible and also unreachable**: the listings filter it out, and
the HTTP handler refuses it on call. Hiding alone is not access control.

## The JSON is the same JSON

The CLI renders reports for a terminal, but it renders **the same erased JSON the HTTP
route returns**. A terminal can never be shown a field HTTP would not return. So a
pipeline can develop against `centinel search x --json` and move to the HTTP route without
re-reading anything.

```bash
centinel list                    # a terminal → prose
centinel list | jq '.sources'    # a pipe → JSON, exactly as before
centinel list --json             # force JSON on a terminal
```

The destination decides the default. `--json` and `--pretty` override the format;
`--color=auto|always|never` overrides colour independently, and `NO_COLOR` is honoured.

Next: [Sources](../operate/sources.md).
