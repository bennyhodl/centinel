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

| Binary | For | Contract |
|---|---|---|
| `poppler` (`pdftoppm`) | rasterising scanned pages | pinned minimum version |
| `tesseract` | OCR | pinned minimum version |
| `yt-dlp` | YouTube acquisition | pinned minimum version |

These are **one-shot subprocesses, not services**. All are **required**.

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

**Safe:** Qwen3 embed + rerank (Apache-2.0), `pdf-inspector` (MIT), LanceDB (Apache-2.0), `sqlite-vec` (Apache-2.0/MIT). Shelling out to GPL poppler is **licence-safe across the process boundary**.

---

## 4. Domain model

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

### 4.1 `Source` is a trait, not an entity with a `kind`

Three implementations differ in `discover`, `fetch`, and `change_signal`. Everything downstream is one shared model. **Variation is quarantined at the acquisition edge**, which is the only place it genuinely exists.

### 4.2 A Resource is an *address*, not a thing in the world

The January 14 council meeting reachable as a Granicus RSS item, a 5.98 MB HTML page, a Legistar Matter, and a YouTube video is **four Resources**. The model makes **no claim** they are the same thing.

*Accepted cost:* nothing knows those four are related; search may return all four.

*Rejected deliberately:* identity resolution across access paths is fuzzy, and **a wrong merge silently corrupts the record** — unacceptable for a transparency tool. Four honest rows beat one confident wrong one.

### 4.3 Document, Transcript, and Sitemap are not entities

- **Derived artifacts are just Blobs.** HTML→markdown, PDF→text+tables, scanned→OCR, audio→transcript are all `Blob → Derivation → Blob`. Content-addressing and version retention apply to derivations for free, and there is **one** re-derivation path.
- **Anchors vary within the Derivation**, not across entities: `(page, bbox, charspan)` for PDFs, time ranges for audio, char spans for HTML.
- **Sitemap** is a `DiscoveryRun` snapshot.

### 4.4 An Observation always has bytes

Failed fetches do not append rows. **`ResourceStatus` carries liveness instead** — failures mutate per-Resource state in place.

This closes a hole successes-only alone would leave: a URL still listed in the sitemap but now 404ing, and — the dangerous one — **a CloudFront/Akamai WAF starting to 403 you**, which would otherwise be indistinguishable from "the site didn't change." Measured live on `phila.gov` and `sec.gov`; a real risk, not hypothetical.

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
  cache/embeddings/              DURABLE  Tier A — (chunk_hash, model_id, dims) → vector
  centinel.db                    DERIVED  SQLite: metadata + FTS5
  index/                         DERIVED  LanceDB: vectors + BM25 + rerank
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

### 5.2 The embedding cache is a durable Tier A artifact

Keyed `(chunk_hash, model_id, dims)`, beside the static files, **not inside any vector store**.

| Tier | Rebuild cost | Portable across backends? |
|---|---|---|
| **A. Embedding cache** | Expensive — inference over the corpus | **Yes — it's just bytes** |
| **B. Search index** | Cheap, minutes | No |

This de-risks §5.1: **swapping vector backends is a re-import, not a re-embed.** A corrupted index is `rm -rf` plus minutes. Adding a model is additive. The cache is also the natural unit to publish — *"Centinel embeddings for cityofX.gov"* lets others build on the crawl without repeating it.

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
  ├─ BM25 (Tantivy, via LanceDB)     → top 100    [instant, no model]
  └─ vector (Qwen3-Embedding-0.6B)   → top 100    [fast]
        └─ RRF fuse (k=60)           → top 30–40
              └─ Qwen3-Reranker-0.6B → top 10     [always on, ~0.5–2 s CPU]
```

### 6.1 Chunk identity is `hash(chunk_text)`

The versioned-corpus problem **dissolves** rather than being solved. A monthly recrawl is ~95% identical, so **only genuinely new chunks are embedded**. Storage is one vector per *distinct* chunk plus a small `(version_id, chunk_hash, ordinal)` join table.

**Two-tier retrieval:** default `is_current = true`; `--as-of <date>` and `--all-versions` reach history. **The tier is a filter, not a copy** — vectors are shared across versions.

### 6.2 Models — fixed for every install

**`Qwen3-Embedding-0.6B` + `Qwen3-Reranker-0.6B`.** Apache-2.0 on both halves, 32K context on both.

Licence was the deciding factor: Centinel auto-downloads models and forks redistribute them. EmbeddingGemma carries the Gemma licence; Jina's reranker is CC-BY-NC. The 32K context also matters concretely — it is the only pair that can embed a long transcript span whole (EmbeddingGemma's 2,048 cannot).

**Fixing the model means every Centinel corpus shares one embedding space.** Had hardware tiering selected embedding models, two installs would produce **incompatible vector spaces** — corpora that could not be compared or merged, fatal when forks are the point.

### 6.3 Reranking is always on

One command, one answer, **no fast path that silently returns worse results.**

This **departs from the research recommendation** to copy qmd's `search`/`query` split. Deliberate: the measured gap is large (BM25Q **14.8 → 33.4** nDCG@10 — reranked BM25 beats an expensively-trained reasoning-tuned dense retriever used alone at 29.1), and a default returning the 14.8 is a footgun.

*Accepted cost:* 0.5–2 s CPU per query. Invisible over MCP; noticeable when iterating on the CLI.

The architectural consequence: **a cheap first stage that over-fetches plus a good reranker beats an expensive retriever alone**, which is why aggressive quantization and MRL truncation are affordable.

### 6.4 Hybrid is the default, not an option

Names, motions, addresses, ordinance numbers, dollar figures — what people actually search meeting records for — are **exact tokens**. Vector-only search would fail hardest on precisely those. *(Inferred; LanceDB provides Tantivy BM25 natively.)*

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

**Store full 1024-dim vectors in the Tier A cache; truncate at index time if ever needed.** Qwen3-Embedding is Matryoshka, so truncation is a *prefix slice* of a stored vector, not a re-embed — making dimension a **reversible index-time decision**. Truncated vectors remain in the same space, so cross-install comparability survives. *(Inferred.)*

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

**Seven decisions remain open.** They are independent of one another and sit on top of §3–§6.

| # | Ticket | Owns |
|---|---|---|
| [#4](https://github.com/bennyhodl/centinel/issues/4) | Crawl scope, boundary & politeness | Build-vs-buy on Firecrawl · site boundary · the per-host UA/rate/contact policy table · robots stance · what is captured vs merely mapped |
| [#7](https://github.com/bennyhodl/centinel/issues/7) | Change detection & scheduling | **The `fingerprint` normalization rules** · trusting vendor `LastModifiedUtc` · when `Live` becomes `Gone` · cadence · idempotency and resumability · the phantom-diff *policy* |
| [#8](https://github.com/bennyhodl/centinel/issues/8) | YouTube as a Source | Whisper tier · audio retention · VAD · metadata change semantics · whether Granicus/Swagit demotes YouTube to a fallback |
| [#9](https://github.com/bennyhodl/centinel/issues/9) | Single-definition → CLI/MCP/HTTP | The generation mechanism · how long operations express themselves across three consumers · what is deliberately not exposed |
| [#11](https://github.com/bennyhodl/centinel/issues/11) | Distribution & packaging | Install channel · whether `cargo install` survives `whisper-rs`'s C++ build · platform matrix · where the version-pin table lives |
| [#12](https://github.com/bennyhodl/centinel/issues/12) | Document extraction pipeline | Routing thresholds · per-page OCR · broken-encoding fallback · table representation · non-PDF formats · failure semantics |
| [#13](https://github.com/bennyhodl/centinel/issues/13) | Hardware profiling & model tiers | What is profiled · whisper tiers · the floor machine · model registry integrity |

Also unspecified: the concrete `centinel.toml` schema, server access control, and the teardown plan for the v1 codebase.

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

The seven open decisions in §8 must be resolved before the system is fully specified. They do not block starting on §3–§6.
