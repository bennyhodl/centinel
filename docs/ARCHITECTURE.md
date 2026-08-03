# Architecture

How Centinel is built. For *why*, see [the README](../README.md); for the settled design decisions and their reasoning, see [`SPEC.md`](SPEC.md).

> **Status: spine.** The domain model, the store, and the CLI/MCP/HTTP derivation are built and tested. Search and retrieval is specified but **not implemented**. Seven design decisions remain open — [`SPEC.md`](SPEC.md) §8 lists them and what each one owns.

## The shape

A **library** first. The CLI, the HTTP server and the MCP server are thin consumers of it. Agents are clients, not the engine.

```
crates/
  centinel-core/    domain model · store · op registry · ops
  centinel-macros/  the #[op] attribute
  centinel/         the binary: CLI, HTTP, MCP
docs/
  SPEC.md           the settled specification — read this first
  research/         ~3,850 lines, ~450 primary-source citations
```

## Files are the only truth

Every index is derived and rebuildable:

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only observations, discovery runs, status, derivations
  current/<source>/…             DERIVED  URL-mirroring tree
  cache/embeddings/              DURABLE  survives an index rebuild
  centinel.db                    DERIVED  SQLite metadata + FTS5
  index/                         DERIVED  LanceDB vectors
```

Delete everything derived and you lose minutes, not evidence. The corpus is `rsync`-able and complete on its own.

Blobs are **pooled across sources** — the same PDF on two `.gov` sites stores once. Logs and trees are **per-source**, so a single city's corpus stays separable for handoff.

### Two hashes, because they answer different questions

| | Computed over | Used for |
|---|---|---|
| `blob_sha` | **raw bytes** | archive identity, CAS filename, evidentiary fidelity |
| `fingerprint` | **normalized content** | "did this meaningfully change?" |

A page whose only variation is a rotated CSRF token yields a new `blob_sha` and an unchanged `fingerprint` — archived faithfully, no change event. Raw-only would produce a new version every recrawl forever; normalized-only would destroy the ability to prove what the server actually served.

Reading a blob **verifies** it still hashes to its address. An in-place edit is an error, not a silent success.

> The normalization rules are currently a deliberately naive whitespace collapse, marked as a placeholder. The real rule set belongs to [ticket #7](https://github.com/bennyhodl/centinel/issues/7).

### A blocked page is not a deleted page

An Observation always has bytes. Failed fetches append nothing — they mutate `ResourceStatus` instead:

| Liveness | Meaning | Trigger |
|---|---|---|
| `Live` | fetched successfully | 2xx |
| `Gone` | authoritatively absent | 404, 410 |
| `Blocked` | refused, but **not** evidence of absence | 401, 403, 429 |
| `Error` | transport or server fault | 5xx, timeout, TLS |

The `Blocked` variant is load-bearing. A CloudFront/Akamai 403 would otherwise be indistinguishable from "the site didn't change" — measured live against real `.gov` hosts. Recording it as `Gone` would log a live page as deleted.

## The domain model

```
Source  (trait — acquisition varies, nothing downstream does)
  ├─ CrawledSite     discover: sitemap + links   id: URL          signal: content hash    (computed)
  ├─ ApiClient       discover: paged query       id: vendor GUID  signal: LastModifiedUtc (asserted)
  └─ YouTubeChannel  discover: playlist          id: video id     signal: metadata revision

DiscoveryRun    full snapshot of the Resource set a run observed
Resource        (source, natural_key) — an ADDRESS
ResourceStatus  Live | Gone | Blocked | Error, + since, consecutive_failures, last_checked
Observation     one successful fetch — ALWAYS backed by a Blob
Blob            content-addressed bytes
Derivation      Blob → Blob edge, carrying tool + version + model tier + anchors
ChangeEvent     materialized index, rebuildable from Observations
```

Two ideas carry most of the weight:

**`Source` is a trait, not an entity with a `kind` field.** The three implementations differ in `discover`, `fetch` and `change_signal`. Everything downstream is one shared model, so variation stays quarantined at the acquisition edge — the only place it genuinely exists.

**A Resource is an *address*, not a thing in the world.** The January 14 council meeting reachable as a Granicus RSS item, an HTML page, a Legistar Matter and a YouTube video is **four Resources**, and the model makes no claim they are the same thing. Identity resolution across access paths is fuzzy, and a wrong merge silently corrupts the record. Four honest rows beat one confident wrong one.

`Document`, `Transcript` and `Sitemap` are **not entities**. Derived artifacts are Blobs linked by a `Derivation` carrying tool, version and model tier — so "the source changed" stays mechanically distinguishable from "tesseract was upgraded". A sitemap is a `DiscoveryRun` snapshot.

## One definition, three surfaces

An op is an ordinary async function. Annotating it puts it on the CLI, in the MCP tool list, and at an HTTP route — with **no central registration list to update**:

```rust
/// List sources in the store with resource counts and liveness.
#[op]
pub async fn list(ctx: &Ctx, args: ListArgs) -> anyhow::Result<ListReport> { … }
```

```console
$ centinel list --max-problems 5              # CLI: flags and help from the same struct
$ curl -X POST localhost:8787/ops/list        # HTTP: JSON in, JSON out
{"jsonrpc":"2.0","method":"tools/list"}       # MCP: JSON Schema from the same struct
```

The registry is what they share:

```
  #[op] async fn search(&Ctx, SearchArgs) -> Result<SearchOut>
        │
        ├── augment_clap ─────────► CLI flags + help text
        ├── schema ───────────────► MCP tool JSON Schema / HTTP request body
        └── invoke ───────────────► one type-erased call path for all three
```

Registration is link-time via `inventory`, so there is nowhere to forget to add an op. The binary names no individual op; it iterates the registry.

**Why a proc macro** rather than build-time codegen or a runtime registry: codegen puts generated source in the tree and makes the definition site not the source of truth; a runtime registry needs an explicit `register(…)` call per op — exactly the central list this avoids, and exactly the thing people forget. *Accepted cost:* proc macros degrade error messages, mitigated by keeping expansion thin.

**Where the mapping is deliberately not mechanical.** Presence is uniform, prose is not. Every op is reachable from all three surfaces unless it opts out of MCP, but each surface renders the same schema in its own idiom.

### Long-running operations

The hardest case. Ops emit progress one way and never learn who called them:

| Surface | Rendering |
|---|---|
| CLI | counter on stderr, so stdout stays a clean JSON stream |
| HTTP | `POST /ops/{name}/stream` → SSE progress frames, then a terminal `result` or `error` |
| MCP | waits and returns once — base MCP has no streaming channel for tool results |

`/stream` holds the connection open rather than returning a job id. Honest for the spine, wrong for a multi-hour crawl; a durable job store belongs with scheduling ([#7](https://github.com/bennyhodl/centinel/issues/7)).

## HTTP surface

| Route | Purpose |
|---|---|
| `GET /health` | liveness |
| `GET /ops` | the registry, with JSON Schema per op |
| `POST /ops/{name}` | invoke, JSON in / JSON out |
| `POST /ops/{name}/stream` | invoke with SSE progress, then the result |
| `POST /mcp` | MCP JSON-RPC over HTTP, sharing the stdio handler |

Op failures are **400, not 500** — nearly every failure reachable here is a bad argument or an unreachable upstream, which is caller-actionable.

**There is no access control**, which is why the default bind is loopback. [`SPEC.md`](SPEC.md) §8 lists server access control as unspecified, and inventing a scheme here would foreclose that decision. Binding to a non-loopback address logs a warning rather than silently exposing the store.

## Try it

```bash
cargo build

# What is this machine missing?
centinel doctor

# What does a city say it has? ~3 seconds for Tampa's 11,476 URLs.
centinel discover --source tampa --site https://www.tampa.gov --rps 3

# Fetch it. --limit lets you try a site before committing an hour.
centinel collect --source tampa --limit 50 --rps 5

# Bytes -> text -> searchable passages
centinel extract
centinel index
centinel search "lobbyist meeting log"

# What is in the store, and what state is it in?
centinel list

# Serve it
centinel serve          # HTTP + MCP over HTTP
centinel mcp            # MCP over stdio
```

`collect` is **resumable with no checkpoint file**: the work list is the latest
`DiscoveryRun` minus everything already observed, so interrupting and re-running picks
up where it stopped. `remaining` in the report is how much is left.

`--match` is a coarse substring filter for exploration — `--match /assets/` pulls just
the documents. For ad-hoc URLs outside a discovery run, `ingest` takes them directly.

Re-collect an unchanged page and it stores the observation, dedupes the blob, and does
not count as changed.

## Requirements

Rust 1.85+. Centinel shells out to standalone binaries rather than running a second language runtime:

| Binary | Needed for | Required |
|---|---|---|
| `pdftoppm` (poppler) | rasterising PDF pages for OCR — Rust cannot do this natively | yes |
| `tesseract` | OCR | yes |
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | audio extraction | no |

`centinel doctor` reports what is missing. Everything runs locally — no OCR, transcription, embedding or reranking leaves the machine. A consequence the whole design carries: **output quality varies by machine**, so the model tier that produced an artifact is part of its provenance.

## Not built yet

Search and retrieval, crawling, YouTube, document extraction, scheduling.

[`SPEC.md`](SPEC.md) §3–§6 are settled and should not be relitigated without reopening the ticket they came from. §8 lists the seven open decisions.
