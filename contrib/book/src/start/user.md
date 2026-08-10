# User

You did not collect this corpus. Somebody handed you a directory, or pointed you at a
machine that serves one. Your job is to ask it questions and be able to check the answers.

You will not need the CLI. There are two surfaces, and they are the same six operations:

- **MCP** — for an agent. Wire it into your client once and the corpus becomes six tools.
- **HTTP** — for a program. JSON in, JSON out, at `POST /ops/{name}`.

Both are served by a process the host runs. Neither can change the corpus.

---

## What you can reach

```
search      Search the corpus for a passage.
read        Read the extracted text of a collected document.
list        List sources in the store with resource counts and liveness.
history     Show what scheduled and manual runs did, newest first.
schedules   Show configured schedules, when each next fires, and how the last one went.
doctor      Report host readiness: required binaries, store location, corpus size.
```

That is the entire remote surface, and the omissions are the design. Every op declares a
**reach** — who may cause it to run — and the remote surfaces honour it:

| Reach | HTTP | MCP | Ops |
|---|:--:|:--:|---|
| `Public` | ● | ● | `search`, `read`, `list`, `doctor`, `schedules`, `history` |
| `Operator` | ✕ | ✕ | `run` and every stage, `ingest`, `source`, `schedule` |
| `Host` | ✕ | ✕ | `open`, `models` |

`Operator` ops change the corpus, and letting a remote caller add a source is letting it
choose the corpus one step earlier. Agents are clients of the record, never its author —
what gets collected does not depend on what any model thought that day. `Host` ops act on
the machine: `open` launches a configured application, `models` pulls gigabytes, and not
even the scheduler may fire them.

A non-public op is **invisible and also unreachable**. It is filtered out of the listing,
and calling it anyway gets the same answer as an op that never existed:

```console
$ curl -s -X POST localhost:8787/ops/run -d '{}'
HTTP 404
{"error":"unknown op `run`"}
```

Hiding alone is not access control, so it is both. See [Ops](../internals/ops.md).

---

## Wiring an agent

### Claude Code

Over stdio, one command:

```bash
claude mcp add centinel -- centinel mcp
```

Point it at a corpus that is not the default `~/.centinel`:

```bash
claude mcp add centinel -e CENTINEL_ROOT=/data/agartha -- centinel mcp
```

Or, against a machine already running `centinel serve`:

```bash
claude mcp add --transport http centinel http://127.0.0.1:8787/mcp
```

Then `claude mcp list` to confirm it connected, and `/mcp` inside a session to see the
tools.

### opencode

opencode has no add command — edit `~/.config/opencode/opencode.json`, or the project's
`opencode.json`. Local, over stdio:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "centinel": {
      "type": "local",
      "command": ["centinel", "mcp"],
      "enabled": true,
      "environment": { "CENTINEL_ROOT": "/data/agartha" }
    }
  }
}
```

Remote, against a running `centinel serve`:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "centinel": {
      "type": "remote",
      "url": "http://127.0.0.1:8787/mcp"
    }
  }
}
```

`opencode mcp list` shows the connection; `opencode mcp debug centinel` diagnoses it.

### Any other client

Claude Desktop, Cursor, Zed and the rest take the same shape. The stdio form is a command
and its arguments:

```json
{
  "mcpServers": {
    "centinel": {
      "command": "centinel",
      "args": ["mcp"],
      "env": { "CENTINEL_ROOT": "/data/agartha" }
    }
  }
}
```

The HTTP form is one URL — `http://127.0.0.1:8787/mcp`.

### stdio or HTTP?

**stdio** is one process per client, started and stopped by the client. Simplest, and right
for a corpus on your own disk.

**HTTP** is one long-lived server that many clients share. Prefer it when more than one
thing asks questions, or when the corpus lives on another machine.

Either way, prefer a *held-open* process to a fresh one per query. Both surfaces load the
embedder and the reranker once and keep them resident; starting a process per question pays
the model load every time — 11 seconds measured with only the 0.6B reranker loaded, more
once the 4B embedder is being built too.

---

## Calling a tool

Every tool's schema is generated from the same Rust struct that defines the op, so the
description your model reads is the description the maintainer wrote:

```json
{
  "type": "object",
  "required": ["query"],
  "properties": {
    "query":         { "type": "string",  "description": "What to search for." },
    "limit":         { "type": "integer", "default": 10,  "description": "Maximum results." },
    "snippet_chars": { "type": "integer", "default": 400,
                       "description": "Characters of matched passage to return. 0 returns the whole chunk." },
    "source":        { "type": ["string","null"], "default": null,
                       "description": "Restrict to one source." }
  }
}
```

A call, and what comes back:

```json
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
  "name":"search",
  "arguments":{"query":"stormwater assessment","limit":1,"snippet_chars":200}}}
```

```json
{
  "method": "bm25→rerank",
  "no_vectors": "no vectors at /data/agartha/vectors.lance — run `centinel embed` first",
  "query": "stormwater assessment",
  "total_chunks_indexed": 11923,
  "vectors_indexed": 0,
  "results": [
    {
      "rank": 1,
      "score": 0.99693763256073,
      "source": "agartha",
      "title": "Stormwater 101 Virtual Town Hall",
      "heading": "Stormwater 101 Virtual Town Hall",
      "text": "[00:05:37] This uh was in place since 2003 uh and it does need to be looked at in terms of increasing the assessment. The other assessment funds our capital projects,…",
      "url": "https://www.youtube.com/watch?v=k7Qm2vXpLn0#captions.json3",
      "blob_sha": "67ebef90b393290d31a88f3c209e9d4416cabc86d6cbabf2d9840222a5bcbb3f",
      "derived_sha": "185c152462e0fdf478b9b9d0f9e674d351f691178dc29f966bba59c91ee0330a",
      "chunk_hash": "1a56c4c9963b189a6f9302bfe39d624800cc5e59fb56350f51e4283af0292bbf",
      "char_start": 4282,
      "char_end": 5516,
      "tool": "youtube-asr-json3 0.1.0",
      "observed_at": "2026-08-04T00:58:48.472639Z"
    }
  ]
}
```

Over MCP that object arrives twice — once as `structuredContent`, once pretty-printed into
`content[0].text` for a model that only reads text. Over HTTP it is the response body,
alone.

**Read the envelope before the results.** Three fields tell you how much of the corpus the
answer could actually see:

`method` is the retrieval path that really ran. The full path is
`bm25 + vector → RRF fuse → rerank`. This one says `bm25→rerank`, and `no_vectors` says
why: the corpus is indexed but not embedded, so only the keyword arm existed. A result set
is never allowed to look complete when half the retrieval was missing. See
[Search](../internals/search.md).

`total_chunks_indexed` and `vectors_indexed` are the two denominators. `vectors_indexed: 0`
against `11923` chunks is a corpus mid-build, and the vocabulary gap is wide open — a water
quality report says `PWSName` and `Analyte`, and no keyword query for *drinking water
sampling results* will ever reach it.

`score` is the **reranker's** score, not a keyword score. A cross-encoder read the passage
against the question and reordered the candidates. That step is worth more than either
retriever — reranked BM25 measures more than twice as good as raw BM25 — which is why there
is no way to turn it off.

---

## The handle is the point

`blob_sha` is a **handle**: the hash of the original bytes the passage came from. Anything
Centinel returns, Centinel takes back — by prefix, git-style.

```json
{"name":"read","arguments":{"target":"67ebef90","max_chars":2000,"offset":4000}}
```

```json
{
  "url": "https://stories.opengov.com/agartha/4f1c7a20-…/published/pQ7r2Xk1?currentPageId=TOC",
  "source": "agartha",
  "kind": "html",
  "tool": "dom_smoothie+htmd 0.18.0+0.5.5",
  "observed_at": "2026-08-07T14:28:38.42691Z",
  "blob_sha": "473cf00bfd80647c7e1b1da029ecf7c7f846f3311d836aa9cd8fd3a7c6d1bd58",
  "derived_sha": "da3b4236f269c857cdf66d379a98550ed2bde02810d99f2bf256b862f32bd36b",
  "chars": 200,
  "total_chars": 12989,
  "offset": 0,
  "truncated": true,
  "text": "# FY2027 Online Budget\n\n*   Table of Contents\n\n*   Overview\n\n*   The Recommended Budget…"
}
```

That header is the provenance of every answer you will ever give from this corpus: which
bytes, from which source, of what kind, **the exact tool and version that derived the
text**, and when it was observed. Reading verifies the hash, so an edited file is an error
rather than a silent success.

Both `blob_sha` and `derived_sha` are valid targets, as is a URL or a substring of one.

**The two useful recipes.**

*Get the context around a hit.* `search` gives `char_start` and `char_end` into the same
extracted text `read` pages through, so read a window around it — `offset: 4000`,
`max_chars: 2000` — and you have what was said either side of the passage.

*Page a long document.* `total_chars` and `truncated` tell you there is more. `max_chars`
defaults to 20,000 rather than unbounded on purpose: a 300-page budget PDF would otherwise
arrive as one tool result and consume the model's whole context.

See [Reading a result](../use/read.md).

---

## Prompts that work

Give the agent the handle discipline and it will use it:

> *Search the corpus for the stormwater assessment increase. For each claim you make, give
> me the handle so I can open the document.*

> *Call `list` first — tell me which sources exist and how fresh they are — then search only
> the one that could plausibly hold a procurement record.*

> *That result is a transcript. Read a window around `char_start` and tell me what was said
> either side of it.*

> *Before you answer, tell me `method` and `vectors_indexed`. If the vector arm did not run,
> say so in your answer.*

What no prompt can do is collect. If the answer is "nothing in the corpus matches", the fix
is an operator adding a source — see [Operator](operator.md).

---

## The HTTP API

```bash
centinel serve --bind 127.0.0.1:8787 --no-schedule
```

| Route | Purpose |
|---|---|
| `GET /health` | liveness — returns `ok` |
| `GET /ops` | the registry, with JSON Schema per op |
| `POST /ops/{name}` | invoke — JSON in, JSON out |
| `POST /ops/{name}/stream` | invoke with SSE frames, then a terminal `result` or `error` |
| `POST /mcp` | MCP JSON-RPC over HTTP, sharing the stdio handler |

```bash
curl -X POST localhost:8787/ops/search \
  -H 'content-type: application/json' \
  -d '{"query":"stormwater assessment","limit":1,"snippet_chars":180}'
```

The body is the same JSON object shown above. `GET /ops` returns every public op with the
schema its arguments are validated against, so a client can discover the surface rather
than hard-code it.

The streaming route ends in one terminal frame either way:

```
event: result
data: {"method":"bm25→rerank","query":"stormwater assessment","results":[…]}
```

**Op failures are 400, not 500.** Nearly every failure reachable here is a bad argument or
an unreachable upstream, which is caller-actionable:

```console
$ curl -s -o /dev/null -w '%{http_code}\n' -X POST localhost:8787/ops/search -d '{}'
400
{"error":"invalid arguments for op `search`: missing field `query`"}
```

**There is no access control**, which is why the default bind is loopback. Binding to a
non-loopback address logs a warning rather than silently exposing the store. A scheme for
this is deliberately unspecified — inventing one here would foreclose the decision.

`serve` also fires the host's configured schedules. `--no-schedule` serves the read API
without them, which is what a machine serving a corpus somebody else collects wants.

---

## One definition, three surfaces

Centinel is a library first. The MCP server, the HTTP server and the operator's CLI are
thin consumers of it, and each verb is a **single annotated Rust function**. The MCP tool
schema, the HTTP request body and the CLI's flags are all generated from that one argument
struct, so they cannot drift, and adding a verb puts it on every surface with no central
list to update.

That is why the JSON above is the same JSON everywhere: a surface can never be shown a
field another surface would not return.

One caveat specific to MCP: tool calls **wait and return once**. Base MCP has no streaming
channel for tool results, so a long op holds the call open. Nothing in the public six is
slow enough to matter, but if a host ever exposes one, use the HTTP streaming route.

---

Next: [Your first corpus](first-corpus.md) if you now want your own, or
[Searching](../use/search.md) for what to put in the query.
