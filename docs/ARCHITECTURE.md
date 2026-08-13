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
  vectors.lance/                 DERIVED  LanceDB chunk vectors        — the vector arm
```

Delete everything derived and you lose no evidence — but not everything derived is cheap.
`centinel.db` rebuilds in minutes; `vectors.lance/` is inference over the whole corpus, so
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

Collection is one process with six stages. Typing them in order is a chore that also has to be got right — `index` before `extract` silently indexes nothing — so the order is written down once, in `centinel.toml`:

```toml
[[source]]
id   = "tampa"
site = "https://www.tampa.gov"

[[source]]
id      = "tampa-council"
channel = "https://www.youtube.com/@CityofTampa"
```

```console
$ centinel run                      # every source: discover → collect → extract → index → embed
$ centinel run --source tampa       # one of them
$ centinel run --skip embed         # stop before the hours-long stage
```

`site` versus `channel` is the *whole* of the website/YouTube difference, mirroring the domain model: the two Source kinds are peers differing only in acquisition, so the config difference is one key and everything downstream is shared.

### The config is intent; the store is fact

They can disagree. Running `centinel discover --source hillsborough --site …` by hand collects a source the config never named — so `run` ignores it, correctly, because nothing declared it. Left there, that is an invisible corpus.

So `source list` reports the **union**, marking what the config does not name:

```console
$ centinel source list
   source        kind  resources             target
✓  tampa         site      1,847             https://www.tampa.gov
   hillsborough  site        412  untracked  https://www.hillsboroughcounty.org

1 source is in the store but not in the config — `centinel run` skips it.
  centinel source adopt
```

Those addresses are **read back out of the log, not guessed**. `DiscoveryRun::method` says `sitemap` or `playlist`, and the resources say where from — provenance recorded for other reasons, answering this. A channel is the interesting case: the log records the videos, never the channel they were listed from, but the archived `yt-dlp -J` document beside each recording carries `uploader_url`. Retaining originals (§5.4) pays for a question nobody had yet.

`centinel source adopt` writes every recoverable one into the config; `centinel source add <id>` with no `--site`/`--channel` does the same for one. A source whose address cannot be recovered is **named and skipped** rather than written as a block that would fail on the next run.

### Two phases, because model loads dominate

```
  per source   discover → collect                      network-bound, per-host paced
  then once    extract → transcribe → index → embed    CPU-bound, model-backed
```

Acquisition is per source because politeness is per host and a 403 on one site must not stop the next. Derivation is corpus-wide because `transcribe` and `embed` each build a multi-gigabyte model — with twenty sources, naive per-source chaining spends more time loading weights than embedding. It also fixes an ordering hazard for free: `index` runs after *every* source has extracted, so a chunk appearing in two sources is placed against both.

A stage whose model is not installed is **skipped, not failed** — an hour of crawling must not be thrown away over a download that was never started, and the stage resumes on the next run once the weights are there. A source that fails is isolated: its remaining stages are skipped, every other source still runs, and the report says which broke.

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
| CLI | progress bars on stderr when stderr is a terminal, plain lines when it is a pipe — so stdout carries only the report, and stays a clean JSON stream whenever it is piped |
| HTTP | `POST /ops/{name}/stream` → SSE progress frames, then a terminal `result` or `error` |
| MCP | waits and returns once — base MCP has no streaming channel for tool results |

A `ProgressEvent` carries an optional **`id`** and a **`unit`**. Events sharing an `id`
are one unit of work, so a renderer can keep a bar per file plus an aggregate beside it
rather than one bar whose meaning shifts underneath the operator; `unit: bytes` is what
turns `312000000/613527539` into `297 MiB / 585 MiB at 18.4 MiB/s`. Both are presentation
hints. The op emits them and never learns whether anything drew a bar.

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
centinel crumbs --rescan                # only for a corpus collected before v0.3

# Serve it
centinel serve          # HTTP + MCP over HTTP
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

Rust 1.85+. Centinel shells out to standalone binaries rather than running a second language runtime. Every call goes through `tool`, the module that owns child processes: each child is killed when its caller is dropped, carries a deadline sized for what it is doing, and never inherits our stdin. `open`'s launcher is the stated exception — it may be somebody's editor, so it takes the terminal and waits.

| Binary | Needed for | Required |
|---|---|---|
| `pdftoppm` (poppler) | rasterising PDF pages for OCR — Rust cannot do this natively | not yet — nothing calls it |
| `tesseract` | OCR | not yet — nothing calls it |
| `yt-dlp` | YouTube acquisition | yes |
| `ffmpeg` | audio extraction | no |

`centinel doctor` reports what is missing. Everything runs locally — no OCR, transcription, embedding or reranking leaves the machine. A consequence the whole design carries: **output quality varies by machine**, so the model tier that produced an artifact is part of its provenance.

## Model weights

```bash
centinel models list                    # registry + what is on disk
centinel models pull                    # ~5.2 GB: embedder + reranker
centinel models pull qwen3-embedding-4b --variant q4_k_m
centinel models verify                  # re-hash against the pinned digests
centinel models prune                   # preview weights the registry dropped
centinel models prune --delete
```

Weights are fetched by an **explicit** `models pull` and never as a side effect
([`SPEC.md`](SPEC.md) §3.2), so a scheduled 3am crawl can fail on a missing model but can
never decide to download a gigabyte on its own.

| | model | default | why this size |
|---|---|---|---|
| Embedding | `qwen3-embedding-4b` | `q8_0` (3.99 GiB) | cost paid **once per corpus**, in hours |
| Reranker | `qwen3-reranker-0.6b` | `q8_0` (0.60 GiB) | cost paid **once per query**, in milliseconds |

**Where the cost lands decides the size** (§6.2). MTEB English Retrieval runs 0.6B
**61.83** → 4B **68.46** → 8B **69.44**: nearly the whole gain is 0.6B→4B, and 8B buys
+1.0 point for roughly double the embedding time. So the embedder stops at 4B and the
reranker is where extra hardware should go, because it never writes a stored artifact.

`qwen3-embedding-0.6b` stays in the registry as a fast smoke-test path, **not** as a
hardware tier — its vectors are 1024-wide against 4B's 2560, a different space entirely,
so switching costs a full re-embed.

**Everything is pinned** — repository, commit revision, and a SHA-256 per file. Pinning
the revision is what makes the digests meaningful: `main` moves, and a digest checked
against a moving target is theatre. GGUF files are self-contained, so a variant is one
file: no sidecar tokenizer, no ONNX external-data pairing.

**Interruption is expected, not exceptional.** Bytes land in `<name>.part` and an
interrupted transfer resumes with `Range: bytes=<n>-` rather than restarting — the same
argument that makes `collect` resumable, for the same reason. Three rules follow:

- a **network error keeps** the `.part`; that retention *is* the resume point
- a **digest mismatch deletes** it, because resuming from known-bad bytes would fail
  identically forever
- a server that **ignores the `Range`** (answers 200 instead of 206) restarts cleanly,
  rather than appending a duplicate prefix that a size check would not catch

The digest is computed from the completed file **on disk**, not from the byte stream —
a stream hash cannot span a resume, because the prefix was written by an earlier process.

Bumping a pin downloads alongside the old revision rather than clobbering it, so an
interrupted upgrade never leaves a half-new model. `models prune` collects what that
leaves behind, and previews by default.

`models` is **host-local**: excluded from MCP and HTTP entirely. It writes outside the
store and can be made to pull gigabytes, which over an unauthenticated server is disk and
bandwidth exhaustion. Weight *status* is reported by `doctor` instead, beside the binary
probes — §3.2 says missing weights are fatal "exactly like a missing binary", and `doctor`
is remotely reachable, so an agent can learn that search is about to fail for want of a
model without being able to trigger a download.

Readiness is split — `binaries_ready`, `models_ready`, `ready` as the conjunction —
because a machine can crawl and extract with no weights at all; it simply cannot search.
`doctor` judges presence **by file size**, so it stays instant; re-hashing 5 GB is
`models verify`'s job.

Weights live in the OS cache directory (`$CENTINEL_MODELS`, else
`~/Library/Caches/centinel/models`), **not** in the store: they are neither corpus nor
provenance, and an `rsync`-able store (§5.4) should not carry gigabytes of GGUF.

## Inference

`llama.cpp` in-process via `llama-cpp-2`. No server, no sidecar. Metal is on by default
on macOS; `--features cuda` / `vulkan` / `rocm` elsewhere, opt-in because they need a
toolchain a plain `cargo build` cannot assume.

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

**Batching is where the win lives.** Unbatched, the same path gives 6.1 chunks/sec —
a context and its KV cache are built per *call*, not per text, so embedding one chunk at
a time measures allocation rather than inference. `cargo run --release --example
embed_bench` reproduces this on any host, which is how a CUDA or DGX Spark box gets
compared without re-deriving a benchmark.

*Accepted cost:* a C++ build enters `cargo build`. Same question ticket
[#11](https://github.com/bennyhodl/centinel/issues/11) already tracks for `whisper-rs`.

**The Qwen3 recipe is not obvious and gets no error when wrong.** Three things a generic
embedding wrapper would not do:

1. **Last-token pooling**, not mean pooling
2. **An instruction prefix on queries only** — documents are embedded bare; the asymmetry
   is the model's
3. **L2 normalization**, so cosine similarity is a dot product

Each produces plausible unit vectors when wrong — slightly worse retrieval, nothing to
catch in a test that checks shapes. So `embed`'s test asserts on *meaning*: a query about
lobbying spend must land measurably closer to a lobbying document than to one about bin
collection.

## Embedding

```bash
centinel embed --dry-run          # what would be embedded, without loading a model
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

```
vectors.lance/          LanceDB, one table: chunk_hash (Utf8) → vector (FixedSizeList<f32, dims>)
```

Two columns and no more. Text and placements stay in `centinel.db`; a second copy goes out
of date the first time the corpus changes, and `chunk_hash` is the join both stores already
use. The model id lives in the table's schema metadata, and a query vector from any other
model is **refused at open** — vectors from two models are in different spaces and still
return a confident ranked list.

There is no separate embedding cache. [`SPEC.md`](SPEC.md) §5.2 specified one and then
reversed it: a `.lance` dataset is an ordinary directory, so `cp -R` backs it up and a
plain scan reads every vector back out. The cache's stated purpose — making a backend swap
a re-import rather than a re-embed — was already true without it.

**Resumability is a consequence, not a feature.** No checkpoint file — the work list is
`index chunk hashes − stored chunk hashes`. Kill it at chunk 40,000 and re-run; it starts
at 40,001. Lance commits a version per append, so what landed before the kill is there. Same shape as `collect`, for the same reason. It is also why a monthly recrawl
is cheap: identical text has an identical `chunk_hash`, so only genuinely new chunks reach
the model (§6.1).

**Batching is not optional.** A batch is one forward pass over many chunks: one context,
one `decode`, every chunk as its own `seq_id`. Two costs collapse into it. A `llama.cpp`
context and its KV cache are built per *call*, not per text — one chunk per call gave 6.1
chunks/sec on an M1 Max against 18.5 for batches of 32, when that amortisation was all a
batch bought. And one ~300-token chunk leaves a GPU almost entirely idle, which is what
the packed pass claims. The batch is the unit of work here, not the chunk. A batch that
fails as a unit — an over-long chunk, or a group the machine cannot hold — is retried
individually, so one bad chunk cannot cost the other 31.

How wide is a property of the machine: `[defaults] embed_batch`, `--batch N` for one run,
or `auto` from the backend's free memory once the weights are on it.

The whole run goes into a single `spawn_blocking`. Inference would otherwise stall the
async runtime, which matters more than usual here because an HTTP caller's connection has
to survive a multi-hour run.

## Why the vector arm exists

Measured on the real corpus. `search "drinking water sampling results"` returns **0 hits**
from FTS5 — the water report says `PWSName`, `Analyte`, `UCMR 5`, and the only chunk
containing "drinking" is a tax table about *Drinking Places (Alcoholic Beverages)*. BM25 is
behaving correctly and is still useless.

That is the case hybrid retrieval is for, and the reason §6.4 makes it the default rather
than an option. It is asserted as a test rather than described:
`ops::search::tests::a_query_reaches_the_passage_that_answers_it` indexes that passage
beside two irrelevant ones, embeds all three, and requires the query to reach it.

## What retrieval reports about itself

```
query
  ├─ BM25   (SQLite FTS5)     → top 100
  └─ vector (Qwen3 + Lance)   → top 100
        └─ RRF fuse (k=60)    → top 40
              └─ Qwen3-Reranker → top n
```

RRF weights by **rank**, and a rank says nothing about the size of the pool it came from —
so a corpus that is 0.6% embedded returns confident results that look exactly like a
complete one's. `search` therefore reports `vectors_indexed` beside `total_chunks_indexed`,
prints the share, and names in `method` exactly which stages ran (`bm25`, `bm25→rerank`,
`bm25+vector→rrf`, `bm25+vector→rrf→rerank`). A stage that did not run says why, in
`no_vectors` or `no_rerank`. Missing weights degrade the answer; they never turn a query
into an error.

## Not built yet

Crawling, YouTube, scheduling.

[`SPEC.md`](SPEC.md) §3–§6 are settled and should not be relitigated without reopening the ticket they came from. §8 lists the six open decisions.
