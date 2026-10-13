# Architecture

How Centinel is built. For *why*, see [the README](../README.md); for the settled design decisions and their reasoning, see [`SPEC.md`](SPEC.md).

> **Status: spine.** The domain model, the store, and the CLI/MCP/HTTP derivation are built and tested. Search and retrieval is specified but **not implemented**. Six design decisions remain open — [`SPEC.md`](SPEC.md) §8 lists them and what each one owns.

## The shape

A **library** first. The CLI, the HTTP server and the MCP server are thin consumers of it. Agents are clients, not the engine.

```
crates/
  centinel-core/    domain model · store · config · op registry · ops · rendering
  centinel-macros/  the #[op] attribute
  centinel/         the binary: CLI, HTTP, MCP
docs/
  SPEC.md           the settled specification — read this first
  research/         ~3,850 lines, ~450 primary-source citations
centinel.toml       what to collect, and where to keep it
~/.centinel/        the store, by default
```

## Files are the only truth

Every index is derived and rebuildable:

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only observations, discovery runs, status, derivations
  current/<source>/…             DERIVED  URL-mirroring tree
  centinel.db                    DERIVED  SQLite metadata + FTS5      — the BM25 arm
  chunks.lance/                  DERIVED  LanceDB chunk vectors        — the vector arm
```

Delete everything derived and you lose no evidence — but not everything derived is cheap.
`centinel.db` rebuilds in minutes; `chunks.lance/` is inference over the whole corpus, so
on 400,000 chunks it is a day. Backing it up is `cp -R`. The corpus is `rsync`-able and complete on its own.

Blobs are **pooled across sources** — the same PDF on two `.gov` sites stores once. Logs and trees are **per-source**, so a single city's corpus stays separable for handoff.

### Where `<root>` is

`~/.centinel`, unless something says otherwise. Nearest answer wins:

| | |
|---|---|
| `--root DIR`, or `$CENTINEL_ROOT` | somebody typed a path — an instruction |
| `root = "~/corpora/tampa"` in `centinel.toml` | the standing preference; `~/` is expanded |
| `~/.centinel` | the default |

In `$HOME` because a store is a corpus you keep, not an artefact of the directory you were standing in. A working-directory default gave every shell its own `.centinel` — each one a separate blob pool, log and index, none of them answering a search against the others, and none of it visible until the corpus turned up empty from one directory up.

The config file is found the same way, nearest first: `$CENTINEL_CONFIG`, then `./centinel.toml`, then `~/.centinel/centinel.toml`, then `~/.config/centinel/config.toml`. A per-project `centinel.toml` still wins, so a checkout travels with its own sources. `centinel source add` writes to whichever of those was found, and to `~/.centinel/centinel.toml` when none was — beside the store the same command collects into.

`centinel doctor` prints both: the root it opened, and the config file that named it.

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
  ├─ SiteSource      enumerate: sitemap          id: URL          signal: content hash    (computed)
  ├─ ChannelSource   enumerate: playlist         id: video id     signal: metadata revision
  └─ ApiClient       enumerate: paged query      id: vendor GUID  signal: LastModifiedUtc (asserted)
                     — not implemented; the shape the first two left room for

DiscoveryRun    full snapshot of the Resource set a run observed
Resource        (source, natural_key) — an ADDRESS
ResourceStatus  Live | Gone | Blocked | Error, + since, consecutive_failures, last_checked
Observation     one successful fetch — ALWAYS backed by a Blob
Blob            content-addressed bytes
Derivation      Blob → Blob edge, carrying tool + version + model tier + anchors
Underivable     a derivation attempted that produced nothing — tool + version + reason
ChangeEvent     materialized index, rebuildable from Observations
```

Two ideas carry most of the weight:

**`Source` is a trait, not an entity with a `kind` field.** Implementations differ in `enumerate`, `acquire` and `change_signal`. Everything downstream is one shared model, so variation stays quarantined at the acquisition edge — the only place it genuinely exists.

The half that does *not* vary lives in `acquire`: one loop that derives the work list from the log, turns refusals into `ResourceStatus`, and keeps the counters, for any Source. `sources::from_config` is the only code that picks an adapter. So `discover` and `collect` are single verbs that name what happens rather than how — there is no `centinel youtube`, and adding a third kind adds no verb.

`acquire` returns a **list** of artifacts rather than one blob, because a video is one address holding metadata, captions and audio, each with its own history. An earlier `fetch(&Resource) -> Fetched` had no possible implementation for that, which is why the kinds routed around the trait instead of through it.

**A Resource is an *address*, not a thing in the world.** The January 14 council meeting reachable as a Granicus RSS item, an HTML page, a Legistar Matter and a YouTube video is **four Resources**, and the model makes no claim they are the same thing. Identity resolution across access paths is fuzzy, and a wrong merge silently corrupts the record. Four honest rows beat one confident wrong one.

An **Observation always has bytes, and so does a Derivation.** Failures on the acquisition side become `ResourceStatus`; failures on the derivation side become `Underivable`. Without the second, "nothing can extract this" is unrecordable, and since every stage computes its work list by subtraction, an audio file gets read and re-attempted on every run for the life of the corpus.

`Document`, `Transcript` and `Sitemap` are **not entities**. Derived artifacts are Blobs linked by a `Derivation` carrying tool, version and model tier — so "the source changed" stays mechanically distinguishable from "tesseract was upgraded". A sitemap is a `DiscoveryRun` snapshot.

## The pipeline

Collection is one process with six stages. Typing them in order is a chore that also has to be got right — `index` before `extract` silently indexes nothing — so `run` does it, over every source the store defines:

```console
$ centinel source add tampa --site https://www.tampa.gov
$ centinel source add tampa-council --channel https://www.youtube.com/@CityofTampa
$ centinel run                      # every source: discover → collect → extract → index → embed
$ centinel run --source tampa       # one of them
$ centinel run --skip embed         # stop before the hours-long stage
```

`site` versus `channel` is the *whole* of the website/YouTube difference, mirroring the domain model: the two Source kinds are peers differing only in acquisition, so a definition differs by one key and everything downstream is shared.

### The store owns the list of sources

A source is defined by `log/<id>/source.toml`, beside the log it feeds, and `crates/centinel-core/src/catalog.rs` is the one module that reads and writes those files. Every surface — the CLI, the web's Sources page, MCP, the scheduler — asks it.

There used to be two lists: `[[source]]` blocks in `centinel.toml`, found by a search path that starts in the working directory, and the store's own `log/` directories. They drifted. A source collected by hand was "untracked" and skipped by `run`; a block removed from the config left its documents searchable; `source adopt` existed only to copy one list into the other. Now, at the start of every command, a legacy block becomes a definition, and so does any log that predates definitions — its address **read back out of the log, not guessed**. `DiscoveryRun::method` says `sitemap` or `playlist`, and the resources say where from. A channel is the interesting case: the log records the videos, never the channel they were listed from, but the archived `yt-dlp -J` document beside each recording carries `uploader_url`. Retaining originals (§5.4) pays for a question nobody had yet.

Schedules stay in the config. A cadence is an intent to collect on its own, and a store handed to someone else should list its sources without crawling them on that person's machine.

### Two phases

```
  per source   discover → collect                      network-bound, per-host paced
  then once    extract → transcribe → index → embed    corpus-wide, server-backed
```

Acquisition is per source because politeness is per host and a 403 on one site must not stop the next. Derivation is corpus-wide. The first reason was model loads: when `transcribe` and `embed` each built a multi-gigabyte model in-process, naive per-source chaining over twenty sources spent more time loading weights than embedding. The models now live in servers that load them once, and the shape stays for the second reason, which it fixes for free: `index` runs after *every* source has extracted, so a chunk appearing in two sources is placed against both.

A stage whose server is not answering — one TCP connect to its `base_url`, before the stage starts — or whose named API key is not set is **skipped, not failed**: an hour of crawling must not be thrown away over a server that was never started, and the stage resumes on the next run once it is up. A server that goes away mid-stage, or refuses the key, ends that stage on the first such answer rather than skipping the corpus one chunk at a time. A source that fails is isolated: its remaining stages are skipped, every other source still runs, and the report says which broke.

### Incremental is inherited, not implemented

Nothing in `run` diffs anything. Every stage already skips work it has done:

| Stage | Work list | Falls out of |
|---|---|---|
| `collect` | latest `DiscoveryRun` − resources already observed | the append-only log |
| `extract` | observations − blobs that already have a derivation | the append-only log |
| `index` | derivations − those already chunked in | the index |
| `embed` | indexed chunk hashes − hashes already cached | the content-addressed cache |

Each is a consequence of files-being-truth rather than a checkpoint file, which is what the storage design in [`SPEC.md`](SPEC.md) §5 was bought for. So a second run does nothing, at every stage, for the same structural reason the first one was resumable — and a re-crawled site is ~95% identical text, so identical chunks hash identically and never reach the model twice (§6.1).

That is what makes this the cron command. Twice a day costs one sitemap walk per source plus whatever actually changed, and a run that found nothing says `nothing new` in one line.

## One definition, three surfaces

An op is an ordinary async function. Annotating it puts it on the CLI, in the MCP tool list, and at an HTTP route — with **no central registration list to update**:

```rust
/// List every source — configured or collected — with resource counts and liveness.
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
        ├── invoke ───────────────► one type-erased call path for all three
        └── render ───────────────► the report, in a terminal's idiom (CLI only)
```

Registration is link-time via `inventory`, so there is nowhere to forget to add an op. The binary names no individual op; it iterates the registry.

**Why a proc macro** rather than build-time codegen or a runtime registry: codegen puts generated source in the tree and makes the definition site not the source of truth; a runtime registry needs an explicit `register(…)` call per op — exactly the central list this avoids, and exactly the thing people forget. *Accepted cost:* proc macros degrade error messages, mitigated by keeping expansion thin.

**Where the mapping is deliberately not mechanical.** Presence is uniform, prose is not. Every op is reachable from all three surfaces unless it opts out of MCP, but each surface renders the same schema in its own idiom.

Each op also declares a **group** — `pipeline`, `stage`, `corpus` or `host` — which decides only the heading it lists under in `centinel --help`. Sixteen verbs in one alphabetical column make `collect`, `embed` and `doctor` look like peer choices, when the first two are steps of what `run` does for you. The group lives on the op rather than in the CLI crate for the same reason registration does: there is nowhere to forget it.

### Reports are rendered, not printed

A report is the right shape for HTTP and for MCP — a model reads JSON better than it reads a table — and the wrong shape for a person, who gets forty lines of quoted keys where four lines would do. So the CLI renders it:

```console
$ centinel list                    # a terminal → prose
$ centinel list | jq '.sources'    # a pipe → JSON, exactly as before
$ centinel list --json             # force JSON on a terminal
$ centinel search x --pretty | less -R    # force prose into a pager
```

The destination decides the default; `--json` / `--pretty` override the format and `--color=auto|always|never` overrides the colour, independently. `NO_COLOR` is honoured and loses only to an explicit `--color always`.

Rendering reads **the same erased JSON `invoke` produced**, so a terminal can never be shown a field HTTP would not return — and a report that `skip_serializing_if` hides from the wire is equally invisible here. That round-trip means every report type must deserialize from its own serialized form, which is a property any Rust consumer of the HTTP API needs anyway.

Each report implements `Render` beside its own definition, and there is **no structural fallback** — a new op will not compile until its report says how it reads. That is the opposite of a central list: forgetting is impossible because the compiler asks at the definition site, in the one place that knows what the numbers mean. What gets dropped is as deliberate as what gets kept — `store_root`, the `action` discriminant and the full digests are all load-bearing on the wire and noise to a person who just typed the command.

### Long-running operations

The hardest case. Ops emit progress one way and never learn who called them:

| Surface | Rendering |
|---|---|
| CLI | log lines on stderr, one per thing done, through `tracing` — so stdout carries only the report, and stays a clean JSON stream whenever it is piped |
| HTTP | `POST /ops/{name}/stream` → SSE progress frames, then a terminal `result` or `error`; every other route and a scheduled run write the same lines to the server's log |
| MCP | waits and returns once — base MCP has no streaming channel for tool results — and the progress goes to stderr, which the client captures |

A `ProgressEvent` carries an optional **`id`** and a **`unit`**. Events sharing an `id`
are one unit of work, so a consumer can tell one file's bytes from the aggregate beside
it rather than reading one counter whose meaning shifts underneath the operator; `unit:
bytes` says that `312000000/613527539` is a size. Both are presentation hints. The op
emits them and never learns who is listening.

Which event becomes which line, and at which level, is decided once, in
`centinel_core::op::log_event`: info is what happened — a page fetched, a document
extracted, a batch embedded, a stage's summary — and debug is what is in flight: the item a
step is about to work on, each tick of a download, the aggregate counter. Beneath that the
internals say what they did at debug — which request was made and what came back, how
long an inference request, a decode or a Lance append took. `-v` selects debug, `-q` keeps only warnings, and `RUST_LOG` overrides both.
The server logs every request at info: method, path and query, status, duration; a search
adds its query, filters, result count and timing; an op invocation adds its arguments, its
outcome and its elapsed time, under a span naming the op and the surface it arrived on.

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

TanStack Start is the public HTTP entry point. Better Auth stores accounts, sessions and OAuth state in local SQLite. The first account becomes an approved administrator; subsequent accounts require approval. Start validates sessions or MCP OAuth tokens and reads current account access on every request. It forwards a trusted role over a private Rust connection protected by a random per-launch credential. Rust enforces read versus admin permissions across HTTP and MCP. Collection and host operations remain local.

`serve` supervises the Node.js process, including startup failure and shutdown. Browser assets and the bundled Start server ship inside the Rust binary; Node.js 22.13+ is required when serving. Remote deployments set `CENTINEL_PUBLIC_URL` to their HTTPS origin, or use `--tailscale`.

## Try it

```bash
cargo build

# What is this machine missing?
centinel doctor

# Name a source, then collect it. --limit tries a site before committing an hour.
centinel source add tampa --site https://www.tampa.gov
centinel run --limit 50

# Ask it something
centinel search "lobbyist meeting log"

# How much is in the store, by kind and by disk?
centinel status

# What is in the store, and what state is it in?
centinel list

# What does it point at that it does not collect? The next source is on this list.
centinel crumbs
centinel crumbs ignore facebook.com     # refuse one, for the life of the corpus
centinel crumbs --rescan                # writes the ledger for a corpus collected before it existed

# Serve it
centinel serve          # HTTP + MCP over HTTP
centinel serve start --tailscale   # the same, as a login service, on the tailnet
centinel mcp            # MCP over stdio
```

Re-run `centinel run` and it says `nothing new`. Put it in cron and that is the steady state — it costs one sitemap walk per source plus whatever changed.

### The stages, individually

`run` performs these in order; each is also its own command, for when you want one of them:

```bash
centinel discover --source tampa --site https://www.tampa.gov --rps 3
centinel collect  --source tampa --limit 50 --rps 5
centinel extract
centinel index
centinel embed
```

`collect` is **resumable with no checkpoint file**: the work list is the latest
`DiscoveryRun` minus everything already observed, so interrupting and re-running picks
up where it stopped. `remaining` in the report is how much is left.

`--match` is a coarse substring filter for exploration — `--match /assets/` pulls just
the documents. For ad-hoc URLs outside a discovery run, `ingest` takes them directly.

Re-collect an unchanged page and it stores the observation, dedupes the blob, and does
not count as changed.

## Requirements

Rust 1.85+, a C compiler and `protoc` to build. Corpus processing shells out to standalone binaries. The authenticated web gateway separately runs Node.js. Every call goes through `tool`, the module that owns child processes: each child is killed when its caller is dropped, carries a deadline sized for what it is doing, and never inherits our stdin. `open`'s launcher is the stated exception — it may be somebody's editor, so it takes the terminal and waits.

| Binary | Needed for | Required |
|---|---|---|
| `pdftoppm` (poppler) | rasterising PDF pages for OCR — Rust cannot do this natively | not yet — nothing calls it |
| `tesseract` | OCR | not yet — nothing calls it |
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | decoding audio for the transcription server | no |

`centinel doctor` reports what is missing, binaries and servers both. OCR runs locally. Transcription, embedding and reranking are requests to the servers `[inference]` names (below); with the default, a server on this machine, nothing leaves it. A consequence the whole design carries: **output quality varies by machine and by server**, so the model tier that produced an artifact — the model, and where it ran — is part of its provenance.

## Inference

Centinel runs no model itself. Embedding, reranking and transcription are each a request
to a server the operator runs, or one they pay for (SPEC §2.1 as amended):

| capability | request | served by |
|---|---|---|
| embed | OpenAI `POST {base_url}/embeddings` | llama-server `--embedding`, Ollama, vLLM, LM Studio, OpenRouter |
| rerank | Cohere/Jina `POST {base_url}/rerank` — OpenAI has none | llama-server `--reranking`, vLLM, TEI, Infinity, LocalAI |
| transcribe | OpenAI `POST {base_url}/audio/transcriptions`, `verbose_json` | `whisper-server --inference-path /v1/audio/transcriptions`, faster-whisper/speaches, LocalAI, OpenAI |

```toml
[inference]
base_url = "http://127.0.0.1:8080/v1"   # default; llama-server's default port
# api_key_env = "OPENROUTER_API_KEY"    # name of the env var, never the key
[inference.embed]
model = "qwen3-embedding-4b"
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
batch = 64          # chunks per request
concurrency = 4     # requests in flight, 1..=32
[inference.rerank]
model = "qwen3-reranker-0.6b"
[inference.transcribe]
model = "whisper-large-v3-turbo"
```

Each capability may name its own `base_url` and `api_key_env`. The file names the
variable that holds a key, never the key: this file is committed, pasted into issues and
printed by `doctor`. A server on `localhost` keeps §2.1's promise that nothing leaves the
machine; a `base_url` elsewhere sends chunk text, queries and audio there, and the config
is the one place that says so — nothing falls back to a server the operator did not
name. The old `[defaults] embed_model`, `embed_batch` and `transcribe_model` keys are
refused with a message saying what to write instead.

The tuning that makes a model fast on one machine — GPU layers, physical batch, flash
attention, threads, VAD — belongs to the server, set once for that machine, and never to
this binary's flags. So there is no GPU build and no model download here: a release is
one plain CPU binary per platform (`centinel-aarch64-apple-darwin`,
`centinel-x86_64-unknown-linux-gnu` built for x86-64-v3,
`centinel-aarch64-unknown-linux-gnu`).

**One failure ends a stage; the rest skip a chunk.** All three go through one HTTP
client: three attempts, the pause doubling from one second, which covers a 429 or a
wobble on the wire. A server that is not listening, a 401 or 403, a 404, a transcription
answer with no timed segments, or 5xx and 429 that outlast the attempts will be answered
the same way by every later request, so those are `Fatal` and stop the stage rather than
skipping a corpus one chunk at a time for hours. Any other 4xx is not retried; a timeout is
retried; neither is fatal, and like a response that fails verification each is left to the
caller, which splits the batch to find the chunk.

**Readiness is a TCP connect.** `Endpoint::reachable()` opens a connection to the
server's host and port and sends nothing, because a request would cost a model call.
`run` uses it to skip a stage whose server is down; `doctor` uses it to report one
`services` row per capability — `base_url`, `model`, `ready`, `problem` — beside the
binary probes, and gates Search on embed and rerank and Transcription on transcribe. A
server not answering stops a stage exactly like a missing binary does, which is why they
sit side by side. Readiness is split — `binaries_ready`, `services_ready`, and `ready` as
their conjunction with a config that loads — because a machine can crawl and extract with
no server at all; it simply cannot search. A config that fails to load is reported as
`config_error` rather than failing the op — over HTTP and MCP, and on the CLI under
`--root`. Without `--root` the CLI reads the config to find the store before any op runs,
so a config that does not load stops every command there, `doctor` included, with the
same message.

The spend ledger files OpenRouter's models under `openrouter/<model>`, the name it has
always used, and every other server's under its own model name with the provider `local`.

### When models ran in-process

Until the `[inference]` block, `llama.cpp` ran in-process through `llama-cpp-2` and
whisper.cpp ran in a second binary, `centinel-whisper`, because the two vendored ggml
copies clashed at link time. Weights came from a pinned registry through
`centinel models pull`, GPU support was a build feature, and `embed --batch auto` sized
batches from the backend's free memory. All of it is gone; the evidence that chose it is
kept here because the default servers still run the same files.

**ONNX was measured and rejected** (§6.2.1). The `onnx-community` exports are decoder
graphs carrying a KV cache, and CoreML refuses them:

```
Input (past_key_values.0.key) has a dynamic shape ({-1,8,-1,128}) but the
runtime shape ({1,8,0,128}) has zero elements. Not supported by the CoreML EP.
```

That made ONNX permanently CPU-only on Apple Silicon. Measured on an M1 Max with
1,200-character chunks, same model on both sides:

| runtime | backend | chunks/sec |
|---|---|---|
| ONNX `ort`, 0.6B int8 | CPU, 10 threads | 5.5 |
| `llama.cpp`, 0.6B Q8_0 | Metal, batch 32 | **18.5** |

**Batching is where the win lived.** Unbatched, the same path gave 6.1 chunks/sec — a
context and its KV cache were built per *call*, not per text, so embedding one chunk at a
time measured allocation rather than inference.

**Where the cost lands decides the size** (§6.2), and that still picks the defaults.
MTEB English Retrieval runs 0.6B **61.83** → 4B **68.46** → 8B **69.44**: nearly the whole
gain is 0.6B→4B, and 8B buys +1.0 point for roughly double the embedding time. So the
default embedder stops at 4B (paid **once per corpus**, in hours) and the reranker is
where extra hardware should go (paid **once per query**, in milliseconds), because it
never writes a stored artifact.

**The Qwen3 recipe is not obvious and gets no error when wrong.** Three things a generic
embedding setup would not do:

1. **Last-token pooling**, not mean pooling — the server's, `--pooling last`
2. **An instruction prefix on queries only** — documents are embedded bare; the asymmetry
   is the model's, and the server cannot know it, so `[inference.embed] query_prefix`
   states it
3. **L2 normalization**, so cosine similarity is a dot product — done client-side, never
   trusted

Each produces plausible unit vectors when wrong — slightly worse retrieval, nothing to
catch in a test that checks shapes.

## Embedding

```bash
centinel embed --dry-run          # what would be embedded, without asking the server
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

```
chunks.lance/           LanceDB, one table: one row per chunk placed in a document —
                        key, chunk_hash, source, resource, derived_sha, tags, excluded, vector
```

No text: that stays in `centinel.db`, and a second copy goes out of date the first time
the corpus changes. The row carries its document and what the workspace decided about it,
so a search's filter runs inside the vector query; `embed` copies a chunk's vector to each
document that holds it rather than embedding it again. The table's schema metadata
records its **space** — the model, and the `base_url` of the server that ran it — and rows
or a query vector from any other space are **refused** — vectors from two spaces still
return a confident ranked list. `search` checks the configured embed server against the
table and, on a mismatch, reports `no_vectors` pointing at `centinel embed --adopt`. That
re-embeds a sample of up to 32 stored chunks on the configured server and adopts it —
one metadata commit, no row touched — only if every pair agrees at cosine ≥ 0.99;
otherwise it refuses with what it measured and what to run instead. A new table takes its
width from one probe request.

There is no separate embedding cache. [`SPEC.md`](SPEC.md) §5.2 specified one and then
reversed it: a `.lance` dataset is an ordinary directory, so `cp -R` backs it up and a
plain scan reads every vector back out. The cache's stated purpose — making a backend swap
a re-import rather than a re-embed — was already true without it.

**Resumability is a consequence, not a feature.** No checkpoint file — the work list is
`index chunk hashes − stored chunk hashes`. Kill it at chunk 40,000 and re-run; it starts
at 40,001. Lance commits a version per append, so what landed before the kill is there. Same shape as `collect`, for the same reason. It is also why a monthly recrawl
is cheap: identical text has an identical `chunk_hash`, so only genuinely new chunks reach
the model (§6.1). The subtraction runs the other way too: before it embeds, a run deletes
the rows whose hash the index no longer has, then compacts the table and drops its
superseded versions — so `embed` owns the table in both directions and a stored vector
means one thing, a row whose hash is in the index.

**A fragment that cannot be read is dropped, not recovered.** A data file is written once;
storage that drops the write leaves a file of the right length with nothing in it, and
every full scan then fails — `embed`'s pre-flight read of the hashes first. `centinel
verify` reads every fragment back and names the ones that fail; `--repair` removes them
from the manifest in one commit against the audited version, so a compaction in between
is a conflict rather than a wrong deletion. The dropped hashes fall back onto the
subtraction above and the next `embed` re-embeds them. `VectorTable` owns both halves;
the op asks, and `embed` only points at it when its scan fails.

**The pipeline is three parts.** A reader thread keeps windows of chunk text coming off
SQLite, `concurrency` requests of `batch` chunks each are in flight to hide the round trip
(`[inference.embed]`, `--batch N` for one run), and a writer task appends what comes back
— so the server waits on neither SQLite nor Lance, and the async runtime is never
blocked, which matters more than usual here because an HTTP caller's connection has to
survive a multi-hour run. A batch that fails is **bisected**, so one chunk the server
will not take costs a dozen requests rather than 64 and is reported as skipped; a `Fatal`
failure is not bisected, because every half would be refused the same way.

## Why the vector arm exists

Measured on the real corpus. `search "drinking water sampling results"` returns **0 hits**
from FTS5 — the water report says `PWSName`, `Analyte`, `UCMR 5`, and the only chunk
containing "drinking" is a tax table about *Drinking Places (Alcoholic Beverages)*. BM25 is
behaving correctly and is still useless.

That is the case hybrid retrieval is for, and the reason §6.4 makes it the default rather
than an option. It is asserted as a test rather than described:
`ops::search::tests::a_query_reaches_the_passage_that_answers_it` indexes that passage
beside two irrelevant ones, embeds all three, and requires the query to reach it. It
needs real servers, so it is opt-in: `CENTINEL_TEST_INFERENCE=1 cargo test`. Without
it, the search tests run against a bag-of-words fake server in `inference/fake.rs`.

## What retrieval reports about itself

```
query
  ├─ BM25   (SQLite FTS5)            → top 100
  └─ vector (embed server + Lance)   → top 100
        └─ RRF fuse (k=60)           → top 40
              └─ rerank server       → top n
```

RRF weights by **rank**, and a rank says nothing about the size of the pool it came from —
so a corpus that is 0.6% embedded returns confident results that look exactly like a
complete one's. `search` therefore names in `method` exactly which stages ran (`bm25`,
`bm25→rerank`, `bm25+vector→rrf`, `bm25+vector→rrf→rerank`), and a stage that did not
run says why, in `no_vectors` or `no_rerank`. A server that is down degrades the answer;
it never turns a query into an error. How much of the corpus the vector arm can see is
`centinel embed --dry-run`'s answer, not the search report's: a per-query chunk count was
99 s cold at 21.7 million chunks, and wrong besides (it ignored the filters and counted
stale vectors).

## Not built yet

Crawling, YouTube, scheduling.

[`SPEC.md`](SPEC.md) §3–§6 are settled and should not be relitigated without reopening the ticket they came from. §8 lists the six open decisions.
