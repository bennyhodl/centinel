# Centinel v2 — Specification

**Status:** partial. Five of twelve decisions locked. Sections 1–6 are **settled and buildable**; §8 lists what is still open. Nothing here is provisional — every decision below was resolved through a wayfinder ticket and carries its reasoning.

**Map:** [MAP: Centinel v2 — data-collection toolkit spec](https://github.com/bennyhodl/centinel/issues/1)
**Evidence:** `docs/research/` — ~3,850 lines, ~450 cited primary sources
**Last updated:** 2026-08-02

---

## 1. What Centinel is

A **generic, config-driven data-collection toolkit** for `.gov` web surfaces and YouTube channels. It ships as a **library**, consumed by a **CLI**, a **server**, and a **derived MCP**.

It collects website maps, PDFs and documents, YouTube channel archives with transcripts, and — critically — the **changes to all of it over time**. Everything is retained, content-addressed, and semantically searchable.

### The reframe

Centinel v1 was built on *"an agent runs everything"* — a pi-coding-agent runtime with five coordinating roles. **That perspective is dropped.** Centinel v2 is a data-collection toolkit. The agent layer is built *around* it and consumes it through MCP.

**Agents are clients, not the engine.**

### Locked scope

| | |
|---|---|
| **Generic, config-driven** | One binary, many targets. Tampa is the first config file, not an assumption. |
| **Consumers** | Agents via MCP · downstream pipelines · the operator on the CLI. **Not** a browsing UI. |
| **Shape** | The library is the core. CLI, server, and MCP are thin consumers of it. |
| **MCP** | **Derived** from the library API, not hand-written. |
| **Server owns** | Scheduled collection · long-running resumable jobs · MCP over HTTP · a read/query API. |
| **Tracking means** | Change detection · every version retained · discovery deltas. |
| **YouTube channels** | First-class Sources, peers of domains. |
| **Licence** | MIT. **Forks are the point** — other cities run their own instance. |

---

## 2. Cross-cutting constraints

These bind every decision below and every decision still open.

### 2.1 Everything runs locally

**No inference leaves the machine.** No OCR API, no transcription API, no embedding API, no reranking API.

Centinel profiles the host and downloads a model tier that fits it. A consequence the whole system carries: **output quality varies by machine**, so the model tier that produced an artifact is part of its provenance.

### 2.2 Static files are the only truth

Everything else — the metadata database, the search index — is **derived and rebuildable**. `rm centinel.db && centinel reindex` must reproduce identical state.

This is the strongest form of the premise, and it forces the append-only log in §5.

### 2.3 No second runtime

Rust plus one-shot subprocess binaries. **No long-lived second-language service.**

Not foreclosed: **the file tree is the contract**, so a sidecar could later read and write the same store without an API being designed for it now. Any decision that requires a Python service is *reopening* this — say so explicitly rather than assuming it.

### 2.4 Provenance is not optional

A search result must be citable back to a specific page of a specific document fetched at a specific time, **by a named tool at a named version**. Without that, a civic-transparency tool cannot support a verifiable claim.

---

## 3. Language and runtime

**Rust.**

### 3.1 Required external binaries

| Binary | For | Contract | Called today |
|---|---|---|---|
| `yt-dlp` | YouTube acquisition | pinned minimum version | **yes** |
| `ffmpeg` | decoding audio to 16 kHz mono PCM | pinned minimum version | **yes** |
| `poppler` (`pdftoppm`) | rasterising scanned pages | pinned minimum version | no — ticket #12 |
| `tesseract` | OCR | pinned minimum version | no — ticket #12 |

These are **one-shot subprocesses, not services**.

The last column is load-bearing for `doctor`. `extract` counts the pages that would need OCR and stops there; neither poppler nor tesseract has a call site. Reporting them as *required* meant a machine able to do everything this code does was told it was **not ready** — and a readiness check that is wrong in the pessimistic direction is not the safe kind of wrong, it is the kind people learn to ignore. They are reported as `planned` until the pipeline that needs them exists.

A fifth subprocess, `centinel-whisper`, is **ours** — built from this workspace, not installed. See §3.6.

Every one of them is started through `centinel_core::tool`, which is the only place in the codebase that runs an external program. Three properties hold for all of them, and none held before that module existed:

- **A child dies with its caller.** `kill_on_drop`, so a cancelled run — Ctrl-C, a lost `select!` race — takes its children with it instead of orphaning an `ffmpeg` and a whisper worker holding a multi-gigabyte model.
- **A child has a deadline**, per call rather than global: a `--version` probe gets seconds, a 63 MB audio download gets half an hour. Exceeding it kills the child.
- **A child never reads our stdin.** An inherited terminal lets a subprocess swallow keystrokes or block on a prompt nobody can see.

For a *stream* the guard is **inactivity**, not total time: a transcription still reporting progress after four hours is working, not stuck. The whisper worker's stderr is therefore both its diagnostics and its heartbeat, and silence past `STALL_TIMEOUT` is what identifies a wedged worker. Model downloads already draw the same distinction — they set a read timeout and deliberately no total-request timeout, because a slow gigabyte is not a stalled one.

The one exception is the application `open` launches, which inherits the terminal and gets no deadline — it may be a person's editor. The interface says so.

`centinel doctor` verifies presence **and pinned minimum version**, and runs before any command. A too-old `tesseract` fails at boot naming the required version — not on page 200 of a crawl. This matters most for `yt-dlp`, which warns at 90 days stale and shipped 26 releases in 2025 in emergency clusters.

### 3.2 Model weights

Fetched by explicit `centinel models pull` into a cache directory. **Missing weights are fatal, exactly like a missing binary** — no multi-GB download ambushes a scheduled 3am run.

### 3.3 Why Rust, given the survey initially said otherwise

- **`firecrawl/pdf-inspector`** (MIT, pure Rust, `lopdf` only) closes the extraction gap the survey called fatal — tables, markdown, reading order, multi-column. It **beats `pymupdf4llm` on every benchmark axis at 36× the speed**, and `pymupdf4llm` was already disqualified as AGPL.
- **Firecrawl's own crawl core is Rust** (`texting_robots`, `lol_html`, `roxmltree`) — the leading commercial crawler built these exact primitives on these crates, in production.
- **Rust's measured crawl gaps total ~450–500 lines** — sitemap parsing (~200–300; `sitemap-rs` is generator-only, the only parser is from 2020) and 429 wiring (~150; primitives present in `spider` but unwired).
- `htmd` is **ahead of** TypeScript's turndown on HTML→markdown, including native tables.
- The one genuine weakness — **rasterise-then-OCR** — is a subprocess in *every* language. Not a Rust-specific cost.

### 3.4 Accepted costs

1. **Highest install bar of the options considered.** Every install, including a fork, needs three binaries at pinned versions plus an explicit model pull.
2. **OCR quality caps around 50/100 on degraded scans** (olmOCR-Bench). 1990s scanned records will not come out clean.
3. **Diarization is unavailable** while there is no sidecar. Speaker attribution stays out of scope.
4. **Docling's chunker is unavailable** (Python-only).
5. **~450–500 lines of gap-filling** must be written rather than depended on.
6. **Two upstream dependencies lag their npm equivalents.** Firecrawl's Rust SDK is batch-ported; `pdf-inspector` is crates.io `0.1.7` against npm `1.11.2`. *Mitigation: depend on the git repo.*
7. **`yt-dlp` staleness is ongoing operational work.**

### 3.5 Licence constraints binding any implementation

| Rejected | Licence | Why |
|---|---|---|
| PyMuPDF, `mupdf-rs`, MuPDF.js | AGPL-3.0 | Centinel ships a server; §13 binds every operator and fork |
| `marker` / `surya` weights | OpenRAIL-M, $5M threshold | Optional backend only, never default |
| Jina reranker | CC-BY-NC-4.0 | Not redistributable |
| `html2md` (Rust), `html2text` (Py), `ultimate-sitemap-parser` | GPL-3.0+ | Permissive substitutes exist |

**Safe:** Qwen3 embed + rerank (Apache-2.0), `pdf-inspector` (MIT), `anydoc` (MIT), LanceDB (Apache-2.0), `sqlite-vec` (Apache-2.0/MIT), whisper.cpp GGML weights (MIT), Silero VAD (MIT). Shelling out to GPL poppler is **licence-safe across the process boundary**.

### 3.6 Transcription runs in a separate process, and it is not optional

`whisper.cpp` and `llama.cpp` each vendor their own copy of **`ggml`**, and both static archives export the same ~534 `ggml_*` symbols. Linked into one binary the linker keeps one copy and silently resolves the other library's calls to it. The two vendored versions are not the same (`ggml.h` is 2,724 lines in whisper.cpp 1.8.3 against 2,845 in llama.cpp via `llama-cpp-2` 0.1.153).

**Measured, identical model and audio, the linked crates the only variable:**

| binary | result |
|---|---|
| `whisper-rs` alone | 2 segments — *"The council meeting will come to order."* |
| `whisper-rs` + `llama-cpp-2` | **0 segments**, every decoded token id 0 at `p=0.000` |

It links without a warning, runs without a crash, and transcribes nothing. There is no error to catch.

**Therefore `centinel` links `llama.cpp` and `centinel-whisper` links `whisper.cpp`, and they meet over a pipe:**

```
blob (m4a/webm) --ffmpeg--> f32le 16 kHz mono --pipe--> centinel-whisper --> JSON segments
```

Both hops stream; a 3-hour meeting is ~691 MB of PCM and is never materialised.

*Rejected:* aligning the two `ggml` versions. It might work today, and the failure mode when it stops working is a silently empty transcript — the one class of bug a transparency tool cannot ship.

*Consistent with §2.3:* a one-shot subprocess, not a long-lived second-language service. The worker is built from this workspace, so it adds no install step beyond what already exists — but it does add a **second C++ build**, which is now the confirmed shape of ticket [#11](https://github.com/bennyhodl/centinel/issues/11) rather than a risk it was tracking.

**VAD is mandatory by default, and this is a §2 decision, not a tuning knob.** Koenecke et al. measured hallucination tracking *non-vocal duration*; a gavel-to-gavel recording is mostly non-vocal. `transcribe` therefore refuses to run without Silero VAD unless `--allow-no-vad` is passed, and records `vad` on every transcript. whisper.cpp **accepts a corrupt or empty VAD model silently**, transcribes without it and exits 0 — verified with `/dev/null` and with 885 KB of `/dev/urandom` — so the worker loads the VAD itself first and refuses rather than let provenance claim a mitigation that never ran.

---

## 4. Domain model

```
Source  (trait — acquisition varies, nothing downstream does)
  ├─ SiteSource      enumerate: sitemap          id: URL          signal: content hash    (computed)
  ├─ ChannelSource   enumerate: playlist         id: video id     signal: metadata revision
  └─ ApiClient       enumerate: paged query      id: vendor GUID  signal: LastModifiedUtc (asserted)
                     — not implemented; the shape the first two were built to leave room for

DiscoveryRun    full snapshot of the Resource set a run observed
Resource        (source, natural_key) — an ADDRESS
ResourceStatus  Live | Gone | Blocked | Error, + since, consecutive_failures, last_checked
Observation     one successful fetch — ALWAYS backed by a Blob
Blob            content-addressed bytes
Derivation      Blob → Blob edge, carrying tool + version + model tier + anchors
Underivable     a derivation attempted that produced nothing — tool + version + reason
ChangeEvent     materialized index, rebuildable from Observations
```

### 4.1 `Source` is a trait, not an entity with a `kind`

Implementations differ in `enumerate`, `acquire`, and `change_signal`. Everything downstream is one shared model. **Variation is quarantined at the acquisition edge**, which is the only place it genuinely exists.

The shared half is `centinel_core::acquire`: one loop that computes the work list from the log, turns refusals into `ResourceStatus`, and keeps the counters — for any Source, whatever its kind. `centinel_core::sources::from_config` is the only code that decides which adapter a `[[source]]` block gets. Consequently there is no `youtube` verb: `discover` and `collect` name what happens, not how.

**`acquire` yields many artifacts, not one blob.** The first shape of this trait was `fetch(&Resource) -> Fetched` — one address, one blob — and nothing could implement it. A video is one address holding up to three artifacts (§4.2), and whether the third is fetched depends on whether the second came back. The kinds went around the trait rather than through it, and their shared machinery got written twice. Returning a list of `(Resource, bytes)` is what makes both kinds expressible through one interface.

**Resumption varies, by exactly one method.** `Source::marker` names the address whose presence proves a Resource was acquired: the page itself for a crawled site, the *metadata* sub-resource for a video. Keying on anything else would re-fetch a whole catalogue every run, because captions and audio may legitimately never exist.

### 4.2 A Resource is an *address*, not a thing in the world

The January 14 council meeting reachable as a Granicus RSS item, a 5.98 MB HTML page, a Legistar Matter, and a YouTube video is **four Resources**. The model makes **no claim** they are the same thing.

*Accepted cost:* nothing knows those four are related; search may return all four.

*Rejected deliberately:* identity resolution across access paths is fuzzy, and **a wrong merge silently corrupts the record** — unacceptable for a transparency tool. Four honest rows beat one confident wrong one.

### 4.3 Document, Transcript, and Sitemap are not entities

- **Derived artifacts are just Blobs.** HTML→markdown, PDF→text+tables, scanned→OCR, audio→transcript are all `Blob → Derivation → Blob`. Content-addressing and version retention apply to derivations for free, and there is **one** re-derivation path.
- **Anchors vary within the Derivation**, not across entities: `(page, bbox, charspan)` for PDFs, time ranges for audio, char spans for HTML.
- **Sitemap** is a `DiscoveryRun` snapshot.

### 4.4 An Observation always has bytes, and so does a Derivation

Failed fetches do not append rows. **`ResourceStatus` carries liveness instead** — failures mutate per-Resource state in place.

This closes a hole successes-only alone would leave: a URL still listed in the sitemap but now 404ing, and — the dangerous one — **a CloudFront/Akamai WAF starting to 403 you**, which would otherwise be indistinguishable from "the site didn't change." Measured live on `phila.gov` and `sec.gov`; a real risk, not hypothetical.

**The same rule binds the derivation side, and needs the same escape.** A `Derivation` is a `Blob → Blob` edge, so it also always has bytes — which leaves no way to record *"this was attempted and produced nothing"*. That gap is not cosmetic: every stage computes its work list by subtraction, and the extract predicate can only be "a Derivation exists", which is never true for an audio file. So every recording in a corpus was read, hashed and re-attempted on every run, forever, to reach the same conclusion.

**`Underivable`** closes it: `from_sha`, `tool`, `version`, `reason`, `at`. Carrying the tool and version is what keeps the verdict honest — it records that **one pipeline at one version** made nothing of these bytes, which is permanent for that version and no claim at all about the next. Bumping the version is how a better extractor gets another go, by exactly the argument §4.6 makes for re-derivation.

### 4.5 `ChangeEvent` is a rebuildable index

Truth is `obs[n-1].fingerprint != obs[n].fingerprint`. The materialized table exists so search can retrieve *over changes* — VersionRAG measured naive RAG at **0% on "what was removed"** queries unless change is an indexed object.

### 4.6 Phantom diffs are solvable because the model carries what is needed

`Derivation` records tool, version, **and model tier**. So *"the source changed"* is mechanically distinguishable from *"tesseract was upgraded"* or *"this ran on a weaker machine with a smaller whisper tier."*

The **policy** — re-extract, extract lazily, or accept a mixed corpus — belongs to change detection (§8).

---

## 5. Storage

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across Sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only:
                                            {observation, resource, blob_sha, fingerprint, at}
                                            {discovery_run, resources[…], at}
                                            {status, Live→Blocked, at, consecutive_failures}
                                            {derivation, from_sha, to_sha, tool, version, tier}

  current/<source>/…             DERIVED  URL-mirroring tree for CLI work. Regenerable.
  centinel.db                    DERIVED  SQLite: metadata + FTS5      — the BM25 arm
  vectors.lance/                 DERIVED  LanceDB: chunk_hash → vector — the vector arm
```

### 5.1 Two embedded stores, both derived

**SQLite** for metadata — no service, first-party Rust, and **FTS5 gives real BM25** (Postgres `ts_rank` does not).

**LanceDB** for vectors — real ANN (IVF/HNSW/PQ/RQ), **Tantivy BM25** so hybrid is native, **first-class reranking**, and **native versioning** (MVCC, ACID, zero-copy time travel, tags, branches). Its own time-travel tutorial uses a **Federal Register** corpus.

**`sqlite-vec` was rejected on a hard number.** It is **brute-force only — no ANN index**. Author's stated ceiling: *"the 100's of thousands."* Cost is linear in `N × d`:

| Corpus | Latency |
|---|---|
| 1M × 128-dim | 33 ms |
| 1M × 192-dim | 192 ms |
| **1M × 3072-dim** | **8.52 s** |

Choosing it would have made **embedding dimension a permanent architectural constraint**. LanceDB removes dimension from the critical path.

*Accepted cost:* two stores rather than one file. Both embedded, neither a service — §2.1 and §3.1 intact.

### 5.2 There is no embedding cache — `embed` writes to LanceDB

**Reversed 2026-08-06.** This section previously specified a durable **Tier A** embedding cache: a flat, append-only file keyed `(chunk_hash, model_id, dims)`, beside the static files and **not inside any vector store**. Its argument was that this de-risked §5.1, because *"swapping vector backends is a re-import, not a re-embed."*

**That argument does not survive measurement.** A `.lance` dataset is an ordinary directory — a manifest, a transaction log, and data files. `cp -R` copies it, the copy opens and queries, and a plain scan reads every vector back out. So:

| the cache was said to buy | what is actually true |
|---|---|
| a backend swap is a re-import, not a re-embed | extracting vectors from Lance **is** the re-import — a table scan |
| the corpus is publishable without repeating the crawl | publishing is a directory copy, and so is backup |
| an interrupted run keeps what it wrote | Lance is ACID with a transaction log, which is **stronger** than truncating a torn record |
| the resume predicate is cheap | reading hashes from a Lance column beats seeking past 4 GiB |

What survived was one property — append-only bytes the query engine never rewrites — against the cost of a second write path, ~4 GiB of duplication, and **a pipeline stage with its own skip predicate**. §7's own table records that each stage's skip predicate has to be exactly right, and a wrong one is the defect that has cost this project the most. The property did not justify the stage.

So: `embed` writes vectors where `search` reads them. One artifact, one write path, no stage.

*Accepted cost:* the vectors now live only inside a store that rewrites itself — compaction, reindexing, version cleanup. Losing `vectors.lance/` costs a full re-embed, which on a 400,000-chunk corpus is roughly a day. **A backup is `cp -R`, and it is the operator's to take.**

*Accepted cost:* models are no longer additive on disk. The cache keyed on `(model_id, dims)`, so several could coexist; one table records its model in schema metadata and **refuses a query vector from any other**. That matches §6.2, which already makes changing the embedder a full re-embed rather than a config edit — but a second model is now a second table someone has to build, not a file that appears beside the first.

### 5.3 Two hashes, because they answer different questions

| | Computed over | Used for |
|---|---|---|
| `blob_sha` | **raw bytes** | archive identity, CAS filename, evidentiary fidelity |
| `fingerprint` | **normalized content** | "did this meaningfully change?" |

A page whose only variation is a rotated CSRF token or "last updated" stamp yields a **new `blob_sha`, unchanged `fingerprint`** — archived faithfully, **no `ChangeEvent`**.

Raw-only would produce a new version every recrawl forever. Normalized-only would destroy the ability to prove what the server actually served — a real evidentiary loss.

**The normalization rules themselves belong to change detection (§8).**

### 5.4 Further

- **Blobs are pooled across Sources; logs and trees are per-Source.** The same PDF on two `.gov` sites stores once; a single city's corpus stays separable for handoff.
- **Original HTML is retained alongside derived markdown.** The past cannot be re-fetched; discarding originals would foreclose re-deriving with a better converter. *(Inferred from §4.3 + §5.2, not separately decided.)*
- The corpus is **`rsync`-able and complete on its own**, matching the forks-are-the-point constraint.

---

## 6. Search and retrieval

```
query
  ├─ BM25 (SQLite FTS5)              → top 100    [instant, no model]
  └─ vector (Qwen3-Embedding-4B)     → top 100    [fast]
        └─ RRF fuse (k=60)           → top 30–40
              └─ Qwen3-Reranker      → top 10     [always on]
```

### 6.1 Chunk identity is `hash(chunk_text)`

The versioned-corpus problem **dissolves** rather than being solved. A monthly recrawl is ~95% identical, so **only genuinely new chunks are embedded**. Storage is one vector per *distinct* chunk plus a small `(version_id, chunk_hash, ordinal)` join table.

**Two-tier retrieval:** default `is_current = true`; `--as-of <date>` and `--all-versions` reach history. **The tier is a filter, not a copy** — vectors are shared across versions.

### 6.2 Models — the Qwen3 family, sized by where the cost lands

**`Qwen3-Embedding-4B` + `Qwen3-Reranker`.** Apache-2.0 on both halves, 32K context on both.

Licence was the deciding factor for the family: Centinel auto-downloads models and forks redistribute them. EmbeddingGemma carries the Gemma licence; Jina's reranker is CC-BY-NC. The 32K context also matters concretely — it is the only pair that can embed a long transcript span whole (EmbeddingGemma's 2,048 cannot).

**Revised 2026-08-03.** This section previously fixed *both* halves at 0.6B, on the reasoning that hardware-tiered embedding models would give two installs incompatible vector spaces — "fatal when forks are the point." That argument assumed a many-install deployment. The actual one is **a single machine that collects, embeds and indexes, with everyone else querying over HTTP or MCP**. There is no second install to be incompatible with, so the constraint that fixed the size is gone. What replaces it is a cost argument.

**Where the cost lands decides the size:**

| | paid | cost of going bigger |
|---|---|---|
| **embedder** | once per corpus | **hours** of wall clock |
| **reranker** | once per query | **milliseconds** |

Measured on MTEB English Retrieval: 0.6B **61.83** → 4B **68.46** → 8B **69.44**. Nearly the whole gain is 0.6B→4B; 8B buys **+1.0 point for roughly double the embedding time**, which on a corpus that takes hours is hours spent on a rounding error.

The 4B is not free either. Measured on an M1 Max under Metal, batch 32:

| | chunks/sec | 200k chunks |
|---|---|---|
| 0.6B Q8_0 | 18.5 | 3.0 h |
| **4B Q8_0** | **3.8** | **14.8 h** |

So the real trade is **+6.6 retrieval points for ~12 extra hours, once**. Taken: retrieval quality is the product, the cost is paid a single time per corpus, and §6.1's `chunk_hash` cache means a monthly recrawl re-embeds only what genuinely changed. But it is an overnight run, not a lunch break, and a first crawl should be planned as one.

Batching helps the 4B far less than the 0.6B (1.4× against 3.0×) because at 4B the matrix arithmetic dominates rather than the per-call setup — there is less overhead left to amortise.

So: **4B for the embedder, and the reranker scales freely with the host.** A reranker emits a score at query time and never writes a stored artifact, so its size is invisible to everything downstream — it is the one place extra hardware converts directly into better answers.

**Model identity is still recorded, and still binding across time.** §5.2's cache key is `(chunk_hash, model_id, dims)`, so several models can coexist on disk — but a query vector and the index it searches must come from the same model. Changing the embedder is therefore a **full re-embed of the corpus**, not a config edit. The cross-install argument is gone; the across-time one is not.

*If the deployment ever becomes many-install* — corpora published and merged between operators, which §5.2 explicitly contemplates for the embedding cache — this decision must be reopened, because the incompatibility it used to prevent returns.

### 6.2.1 Runtime — GGUF via `llama.cpp`, not ONNX

Model **weights** are GGUF; inference is `llama-cpp-2` in-process. No server, no sidecar.

**ONNX was measured and rejected.** The `onnx-community` exports are decoder graphs carrying a KV cache (28 layers × key/value, plus `position_ids`), and CoreML refuses them:

```
Input (past_key_values.0.key) has a dynamic shape ({-1,8,-1,128}) but the
runtime shape ({1,8,0,128}) has zero elements. Not supported by the CoreML EP.
```

That makes ONNX **permanently CPU-only on Apple Silicon**. `llama.cpp` has first-class Metal, CUDA, Vulkan and ROCm backends, and GGUF is the format quantization ladders are actually published in.

Measured on an M1 Max, 1,200-character chunks, **same model on both sides** so the runtime is the only variable:

| runtime | backend | chunks/sec |
|---|---|---|
| ONNX `ort`, 0.6B int8 | CPU, 10 threads | 5.5 |
| `llama.cpp`, 0.6B Q8_0 | Metal, batch 32 | **18.5** |

**3.4×, and only with batching.** Unbatched the same path gives 6.1 chunks/sec — barely better than CPU — because a context and its KV cache are built per *call*, not per text. Embedding one chunk at a time measures allocation, not inference. Any consumer of [`crate::embed`] that loops one text at a time is leaving two thirds of the throughput on the floor.

*Known headroom:* the batched path still decodes sequences one at a time inside a shared context. True multi-sequence batching — several `seq_id`s in one `LlamaBatch` — is not implemented.

*Accepted cost:* a C++ build enters `cargo build`. Ticket [#11](https://github.com/bennyhodl/centinel/issues/11) already tracked this for `whisper-rs`; both are now present, and §3.6 explains why they cannot share a binary — so #11 decides packaging for **two** C++ builds producing **two** executables.

*Accepted cost:* the reranker has **no first-party GGUF** — Qwen publishes GGUF for the embedder only. Reranker weights come from a community conversion (`ggml-org`, the llama.cpp organisation). This is not a change in provenance: the ONNX weights were community conversions too. Digests pin exactly what is fetched either way; what is weaker is the chain of custody, and it should be recorded in the registry rather than glossed.

### 6.3 Reranking is always on

One command, one answer, **no fast path that silently returns worse results.**

This **departs from the research recommendation** to copy qmd's `search`/`query` split. Deliberate: the measured gap is large (BM25Q **14.8 → 33.4** nDCG@10 — reranked BM25 beats an expensively-trained reasoning-tuned dense retriever used alone at 29.1), and a default returning the 14.8 is a footgun.

*Accepted cost:* around a second per query. Invisible over MCP; noticeable when iterating on the CLI.

The architectural consequence: **a cheap first stage that over-fetches plus a good reranker beats an expensive retriever alone**, which is why aggressive quantization and MRL truncation are affordable — and why §6.2 spends its hardware budget on the reranker rather than the embedder. The first stage only has to get the right document into the top 100; it does not have to rank it.

### 6.4 Hybrid is the default, not an option

Names, motions, addresses, ordinance numbers, dollar figures — what people actually search meeting records for — are **exact tokens**. Vector-only search would fail hardest on precisely those.

The BM25 arm is **SQLite FTS5**, not LanceDB's Tantivy. §5.1 listed both; FTS5 is built, tested, and already demonstrated on the real corpus, and RRF fusion in our own code is a few dozen lines. That keeps the two arms independent — either can be rebuilt without touching the other.

### 6.5 Chunking — three shapes, three pipelines

| Shape | Approach |
|---|---|
| **Markdown pages** | Heading-aware structural chunking |
| **PDF-extracted text** | Page boundaries preserved; anchors carried |
| **Meeting transcripts** | See below — bespoke |

**Transcripts: opportunistic agenda alignment.** When an agenda PDF matches confidently (timestamp heuristics plus lexical alignment), transcript spans chunk on **agenda items**; otherwise, semantic topic-shift chunking (the TextTiling family).

This was the highest-value idea in the search research, available to Centinel **precisely because it also harvests the agenda PDFs** — it cross-links two of three corpora and gives structure to a document that has none. Opportunistic so meetings without an agenda degrade gracefully.

Binding on the implementation:

- **Store timestamps per chunk unconditionally.** They are the transcript's page numbers — they turn a hit into a `watch?v=X&t=4271s` citation, which is the entire value proposition.
- **Restore punctuation before chunking** if ASR output lacks it, or sentence-boundary splitting degenerates to arbitrary splitting.
- **Prefer large chunks (1,000–1,500 tokens).** Procedural filler dominates; smaller chunks produce more pure-filler chunks.
- **There is no library to adopt.** The transcript pipeline is bespoke work.

### 6.6 A result is a ranked passage with full provenance

```
hit {
  text, score,
  source_url, observed_at, blob_sha, version,
  anchor: (page 47, bbox) | (t=4271s → deep link),
  derived_by: { tool, version, model_tier }
}
```

Multiple hits from one document appear separately. Every hit is **independently verifiable** — see §2.4.

### 6.7 Dimensions

**Store vectors at full width; truncate at query time if ever needed.** Qwen3-Embedding is Matryoshka, so truncation is a *prefix slice* of a stored vector, not a re-embed — making dimension a **reversible index-time decision**. Truncated vectors remain in the same space, so cross-install comparability survives. *(Inferred.)*

---

## 7. Evidence base

`docs/research/` — four primary-source streams, ~450 cited sources.

| File | Covers |
|---|---|
| `crawling-and-sitemaps.md` | Firecrawl, per-language crawlers, WAF/429 ground truth, government CMS APIs |
| `pdf-and-ocr.md` | Extraction quality, OCR, licence hazards, provenance anchoring |
| `semantic-search.md` | qmd, pgvector vs embedded, embeddings, reranking, versioned corpora |
| `youtube-and-transcription.md` | Quota, caption fragility, Whisper cost, hallucination |

**Findings that materially shaped this spec:**

- **Legistar's Web API is keyless and OData-queryable, with a server-side-filterable `LastModifiedUtc`** — a vendor-supplied change feed. ArcGIS Hub, PrimeGov, Municode, and Granicus RSS are comparable. *For the highest-value content, do not crawl — query.*
- **The `.gov` blocking mode is a WAF 403 without `Retry-After`, not a 429.** The fix is a descriptive User-Agent and a **per-host policy table** that no library provides.
- **`qmd` is [`tobi/qmd`](https://github.com/tobi/qmd)** — 28.5k stars, MIT, a single SQLite file running FTS5 + `sqlite-vec` + local embedding + local reranking with zero servers.
- **[GovScape](https://arxiv.org/abs/2511.11010)** indexed 10M federal PDFs / 71M pages for **~$1,500 total compute**. Closest published precedent.
- **Whisper hallucination correlates with non-vocal duration** (Koenecke et al., FAccT 2024): ~1% of transcriptions fabricate, 38% of those harmfully. Council recordings are mostly dead air, and the mitigations are off by default.

---

## 8. Not yet specified

**Six decisions remain open.** They are independent of one another and sit on top of §3–§6.

| # | Ticket | Owns |
|---|---|---|
| [#4](https://github.com/bennyhodl/centinel/issues/4) | Crawl scope, boundary & politeness | Build-vs-buy on Firecrawl · site boundary · the per-host UA/rate/contact policy table · robots stance · what is captured vs merely mapped |
| [#7](https://github.com/bennyhodl/centinel/issues/7) | Change detection & scheduling | **The `fingerprint` normalization rules** · trusting vendor `LastModifiedUtc` · when `Live` becomes `Gone` · cadence · idempotency and resumability · the phantom-diff *policy* |
| [#8](https://github.com/bennyhodl/centinel/issues/8) | YouTube as a Source | ~~Whisper tier~~ (§3.6, registry) · ~~VAD~~ (§3.6, mandatory) · audio retention · metadata change semantics · whether Granicus/Swagit demotes YouTube to a fallback · **the bot wall** — measured to persist across yt-dlp 2026.03.17 → 2026.07.04, so it is the IP and not a stale extractor: whether cookies or a PO-token provider are in scope, or a blocked day is simply a blocked day |
| [#11](https://github.com/bennyhodl/centinel/issues/11) | Distribution & packaging | Install channel · whether `cargo install` survives **two** C++ builds · platform matrix · where the version-pin table lives · that Centinel now ships as two executables (§3.6) |
| [#12](https://github.com/bennyhodl/centinel/issues/12) | Document extraction pipeline | Routing thresholds · per-page OCR (**never executed** — `pdftoppm` absent) · broken-encoding fallback · table representation · ~~DOCX/PPTX~~ — answered by `anydoc` (MIT, pure Rust, wraps `pdf-inspector` for PDFs and is **not** used for them; see the correction in `research/pdf-and-ocr.md`) |
| [#13](https://github.com/bennyhodl/centinel/issues/13) | Hardware profiling & model tiers | What is profiled · the floor machine · whether better hardware replaces or versions an artifact. ~~Model registry integrity~~ — pinned by repo + revision, checksum-verified, licence recorded |

[#9](https://github.com/bennyhodl/centinel/issues/9) — the single-definition mechanism — is **closed**. Its decision is recorded in `crates/centinel-core/src/op.rs` rather than here, since the module doc sits beside the code it constrains: a `#[op]` macro with link-time registration via `inventory`, one args struct deriving both `clap::Args` and `JsonSchema`, presence uniform across surfaces while prose is not, and `local_only` enforced on call rather than merely omitted from `tools/list`.

The **`centinel.toml` schema** is now partly settled, and its decision is recorded in `crates/centinel-core/src/config.rs` rather than here, beside the code it constrains. What is fixed: `[[source]]` blocks carrying `id` plus exactly one of `site` or `channel` — the §4.1 claim that the two Source kinds differ only in acquisition, spelled as one key; a `[defaults]` table each source may override; `[open]` as before; a top-level `root` naming the §5 store root, defaulting to `~/.centinel` so a corpus belongs to the operator rather than to the directory the binary was started from; and unknown keys rejected rather than ignored, because `[[sources]]` typed by reflex would otherwise parse cleanly and collect nothing.

What that does **not** settle is scheduling. `centinel run` walks the sources and every stage skips work it has already done, so a cron entry is enough for cadence — but *when* to recrawl, when `Live` becomes `Gone`, and whether a vendor `LastModifiedUtc` can be trusted instead of a crawl all still belong to [#7](https://github.com/bennyhodl/centinel/issues/7).

Also unspecified: server access control, and the teardown plan for the v1 codebase.

---

## 9. Out of scope

| | Why |
|---|---|
| **Web app, viewer, dashboards** | This spec covers library, CLI, server, and MCP only. (Markdown is a *storage format*, not a browsing surface.) |
| **A curated wiki of findings** | v1's human-facing output model. Consumers are agents, pipelines, and the CLI. |
| **Semantic diffs** | Understanding *what* a change means rather than *that* it happened. |
| **Speaker attribution & agenda-segment structure in transcripts** | Diarization is Python-only and §2.3 forbids a sidecar. Timestamps **are** in scope; speaker identity is not. Reopening this reopens §2.3. |
| **Migrating existing collected data** | There is none — v1's `wiki/` was empty scaffolding. |
| **The pi-coding-agent role/persona layer** | Superseded by the reframe in §1. |

---

## 10. Reading this as a builder

Sections 3–6 are decided. **Do not relitigate them** — each carries its reasoning and its accepted costs, and the research backing them is in `docs/research/`.

If implementation reveals a decision here is wrong, that is a real finding: say so explicitly, name which section, and reopen the ticket it came from. Silent drift away from this document is the failure mode it exists to prevent.

The six open decisions in §8 must be resolved before the system is fully specified. They do not block starting on §3–§6.
