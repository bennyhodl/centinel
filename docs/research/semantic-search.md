# Semantic Search & Vector Storage — Research

Frame: static files on disk are the **source of truth**; the vector index is a
**derived, rebuildable artifact**. Language (Rust / Python / TypeScript) is
UNDECIDED — this document flags which choices constrain it and which don't.

## TL;DR

1. **`qmd` is [`tobi/qmd`](https://github.com/tobi/qmd)** — a local hybrid-search CLI over
   markdown. Its stack is **one SQLite file**: FTS5/BM25 + sqlite-vec + EmbeddingGemma-300M
   + Qwen3-Reranker-0.6B, all in-process via `node-llama-cpp`, fused with RRF. No server.
   **The operator's stated preference (pgvector) and stated design reference (qmd) point in
   opposite directions on storage** — §1, §2.
2. **Don't make Postgres a hard dependency.** One storage interface, two backends: embedded
   (sqlite-vec or LanceDB) by default, pgvector for the server. A hash-keyed **embedding
   cache** makes them interchangeable — §8.
3. **Hybrid, not pure vector.** On hard out-of-domain retrieval BM25 (13.7 nDCG@10 on
   BRIGHT) ties a strong general embedding model (BGE-large, 13.8), and government text is
   full of exact tokens. The "BM25 tops long-context benchmarks" claim is **not literally
   true** but the useful version of it is — §5.
4. **Reranking is the biggest measured win in this document: +18.6 nDCG@10** on top of
   BM25. It beats switching to an expensive retriever — §6.
5. **Content-hash chunk identity** turns the versioned corpus from a 20× vector explosion
   into ~1.6×, and makes incremental reindex free — §4.
6. **Model "what changed between versions" as an indexed object.** Naive RAG scores **0 %**
   on implicit-change queries; similarity search cannot find absent text — §4.
7. **Nothing in the search layer decides the language.** Embedding, reranking, and every
   candidate store are reachable from all three — §9.

---

## 1. `qmd` — what it actually is

**Resolved: it is [`tobi/qmd`](https://github.com/tobi/qmd)** — "mini cli search engine for
your docs, knowledge bases, meeting notes, whatever. Tracking current sota approaches
while being all local." MIT licensed, ~28.5k stars, ~582 commits, written in TypeScript,
runs on Node ≥22 or Bun ≥1.0. Author is Tobi Lütke.

It is **not** Quarto `.qmd` literate-programming documents. Quarto `.qmd` files are a real
thing (a Pandoc-markdown variant with executable code cells), but nothing about "I use qmd
for markdown stuff and like how it works" in a *search* conversation points there. The
operator is describing a local hybrid-search CLI over a markdown corpus — which is almost
exactly Centinel's problem statement. Treat `tobi/qmd` as the design reference.

Note also the repo tagline explicitly names **"meeting notes"** as a target corpus. That is
the transcript problem (§7) already in scope for the reference implementation.

### Architecture, in detail

| Concern | qmd's choice |
| --- | --- |
| Storage | **A single SQLite file** at `~/.cache/qmd/index.sqlite` (respects `XDG_CACHE_HOME`) |
| Schema | `collections`, `path_contexts`, `documents`, `documents_fts` (FTS5), `content_vectors`, `vectors_vec` (sqlite-vec), `llm_cache` |
| Keyword index | **SQLite FTS5 / BM25** |
| Vector index | **`sqlite-vec`**, shipped as pre-compiled per-arch binaries (darwin-arm64, linux-x64, windows-x64, …) as optional npm deps |
| Embedding model | **`embeddinggemma-300M-Q8_0.gguf`** (~300 MB), run in-process via `node-llama-cpp`; auto-downloads from HuggingFace on first use. Override with `QMD_EMBED_MODEL` (e.g. `Qwen3-Embedding-0.6B` for CJK/multilingual) |
| Embedding prompt | Documents: `"title: {title} \| text: {content}"` · Queries: `"task: search result \| query: {query}"` |
| Reranker | **`Qwen3-Reranker-0.6B-Q8_0`** cross-encoder, also local via `node-llama-cpp`, over the top ~30–40 candidates |
| Query expansion | A third local model (**Qwen3-1.7B**) rewrites the query into typed variants: `lex:` (BM25 keywords), `vec:` (semantic rephrasings), `hyde:` (hypothetical answer document) |
| Fusion | **Reciprocal Rank Fusion, k=60**, position-aware blend with the reranker: rank 1–3 → 75 % retrieval / 25 % reranker; 4–10 → 60/40; 11+ → 40/60 |
| Total model footprint | ~300 MB to ~1.1 GB across the three GGUF models |

### Chunking (directly relevant to §7)

Not fixed-size. qmd targets **~900 tokens with 15 % overlap** and picks the *highest-scoring
structural break point within a 200-token window* before the target. Break points are
scored: **headings 50–100, code fences 80, blank lines 20**. For code files
(`.ts .js .py .go .rs`) it can do AST-aware chunking via `web-tree-sitter` under
`--chunk-strategy auto`; tree-sitter grammars are optional deps and it falls back to
regex-only chunking if they're missing.

This is the single most transferable idea in qmd: **scored structural boundaries with a
token budget and a search window**, rather than either naive fixed-size splitting or
purely-structural splitting that produces wildly uneven chunks.

### Incremental reindex

- Documents get a **content hash**; the docid is the **first 6 chars of that hash**, so
  references stay stable across filesystem moves.
- `qmd update` rescans the filesystem, diffs against stored hashes, and **re-embeds only
  changed content**.
- Collections can declare an `update:` command in `index.yml` (e.g. `git pull`) that runs
  before reindexing.

Content-hash-keyed chunks is exactly the right primitive for Centinel, whose file tree is
already hash-addressed. If chunk identity is `hash(chunk_text)`, then re-crawling a page
whose boilerplate is unchanged re-uses most of its existing embeddings for free — see §4.

### CLI ergonomics

```
qmd search  "query"          # BM25 keyword only — fast default
qmd vsearch "query"          # vector-only semantic
qmd query   "query"          # hybrid + expansion + rerank (best quality)
qmd collection add ~/path --name label
qmd embed                    # generate vectors
qmd update                   # incremental reindex
qmd get "file.md" | qmd get "#docid"
qmd context add qmd://collection "description"
```

Output: `--json --csv --md --xml --files`, plus a colorized default with TTY hyperlinks.
It also exposes an **MCP server over stdio or HTTP** so agents can query the index and
fetch documents — the same library/CLI/server/MCP shape Centinel is planning.

**Three separately-named search commands is a deliberate ergonomic choice worth copying.**
BM25 is the cheap instant default; semantic and full-hybrid are opt-in because they cost
model time. Centinel should not make every query pay for a reranker.

### Ecosystem signals

- [`ba0f3/qmd-go`](https://github.com/ba0f3/qmd-go) — a Go port exists, evidence the
  architecture ports cleanly across languages (it is SQLite + two model files, nothing
  runtime-specific).
- [`achekulaev/obsidian-qmd`](https://github.com/achekulaev/obsidian-qmd) — Obsidian plugin.
- [`lazyqmd`](https://alexanderzeitler.com/articles/introducing-lazyqmd-a-tui-for-qmd/) — TUI.

### What this implies for Centinel

qmd is a **direct existence proof that the whole stack the operator wants — semantic search,
hybrid BM25 fusion, cross-encoder reranking, local models, incremental reindex — fits in one
SQLite file with zero servers.** No Postgres. It is worth being explicit that the operator's
stated preference (pgvector) and the operator's stated reference implementation (qmd) point
in *opposite* directions on the storage question. See §2 and §3.

## 2. pgvector — honest assessment

[github.com/pgvector/pgvector](https://github.com/pgvector/pgvector) — current version
**0.8.6**, PostgreSQL-licensed, available everywhere (Docker, Homebrew, APT, conda-forge,
and preinstalled on essentially every managed Postgres).

pgvector is genuinely good software. The question is not whether it works — it is whether
**Centinel should require a Postgres server**.

### Types and dimension ceilings (a real constraint)

| Type | Max dims | Storage per row | HNSW max dims |
| --- | --- | --- | --- |
| `vector` (fp32) | 16,000 | `4·d + 8` bytes | **2,000** |
| `halfvec` (fp16) | 16,000 | `2·d + 8` bytes | **4,000** |
| `bit` | 64,000 | `d/8 + 8` bytes | 64,000 |
| `sparsevec` | 16,000 nonzero | `8·nnz + 16` bytes | 1,000 nonzero |

**The HNSW 2,000-dim ceiling on `vector` bites.** Several strong models exceed it:
OpenAI `text-embedding-3-large` (3072), Qwen3-Embedding-8B (4096), many 3072-dim open
models. You must drop to `halfvec` to index them at all — which is usually fine (fp16
costs ~1 % recall in practice and halves storage) but it is a real, surprising constraint
you should know about before choosing a model.

### Index types

- **HNSW** — `m` (default 16), `ef_construction` (default 64); query-time
  `hnsw.ef_search` (default 40). Better recall/latency, slower and far more
  memory-hungry to build.
- **IVFFlat** — `lists` (guidance: `rows/1000` up to 1 M rows, `sqrt(rows)` beyond);
  query-time `ivfflat.probes` (default 1). Much cheaper to build, but **must be built on
  already-populated data** and degrades as the table drifts from the trained centroids —
  bad for an index you rebuild incrementally, fine for one you rebuild wholesale.

Six distance metrics (L2, inner product, cosine, L1, Hamming, Jaccard), binary
quantization, and **iterative index scans** (added in 0.8.0) which materially improve the
filtered-search case — relevant here because Centinel will constantly want
`WHERE domain = ... AND crawled_at > ...`.

### Build cost and memory — the honest numbers

pgvector's own guidance: *"Indexes build significantly faster when the graph fits into
`maintenance_work_mem`."* That is the whole story. Rough HNSW build-memory estimate
circulating in practitioner writeups is `N · d · 4 bytes · 2` (the 2× is graph overhead);
**5 M rows × 1536 dims ≈ 60 GB**. At 10 M × 1536, the vector column alone is ~60 GB and
the HNSW index lands around **80–120 GB**
([dev.to writeup](https://dev.to/philip_mcclarence_2ef9475/scaling-pgvector-memory-quantization-and-index-build-strategies-8m2)).
Parallel build (default 2 workers, raise `max_parallel_maintenance_workers`) cuts build
time roughly 30–50 % on multi-core.

**Practical ceiling: ~10 M vectors per node** before you are fighting the buffer pool;
`pgvectorscale` pushes that to roughly **50 M at p95 < 50 ms**. For scale calibration:
a mid-size city's `.gov` surface — say 200 k pages/PDFs, ~10 chunks each, one version —
is ~2 M vectors. Comfortably inside pgvector's envelope. **But multiply by version count
(§4) and the picture changes fast.**

### Hybrid search — the weak spot

pgvector composes with Postgres FTS (`tsvector` + GIN), and this is the standard recipe:
run both, fuse with RRF or a weighted sum. It works. Two honest caveats:

1. **Postgres `ts_rank`/`ts_rank_cd` is not BM25.** It is a length-normalized
   term-frequency score with no corpus-level IDF the way BM25 defines it
   ([Postgres FTS controls docs](https://www.postgresql.org/docs/current/textsearch-controls.html)).
   On a corpus of hundreds of thousands of government documents with heavy boilerplate
   repetition, IDF is exactly what you need. SQLite's FTS5 ships real BM25 out of the box;
   Postgres does not.
2. Fusing two index scans in one SQL statement with sane limits is fiddly (the classic
   CTE-per-arm + `FULL OUTER JOIN` + RRF pattern). It's a known-good pattern, just not free.

### The extensions that change the picture

| Extension | What it adds | Status |
| --- | --- | --- |
| **[pgvectorscale](https://github.com/timescale/pgvectorscale)** (Timescale/Tiger) | StreamingDiskANN index + statistical binary quantization; disk-friendly, extends practical scale to ~50 M/node | Rust/pgrx, actively maintained |
| **[VectorChord](https://github.com/tensorchord/VectorChord)** (TensorChord) | **Explicit successor to `pgvecto.rs`**; RaBitQ4 / RaBitQ8 quantized vector types (RaBitQ8 claims <1 % recall loss), new disk-backed graph index, much faster builds. **Depends on pgvector and is drop-in compatible** | Actively developed; `tensorchord/vchord-suite:pg17-latest` Docker image |
| **[pgvecto.rs](https://github.com/tensorchord/pgvecto.rs)** | — | **Superseded. Do not start here.** Use VectorChord. |
| **[ParadeDB `pg_search`](https://github.com/paradedb/paradedb)** | **Real BM25 inside Postgres**, built on **Tantivy** via pgrx. Faceting, aggregations. Fixes caveat (1) above | v0.22.5 (Apr 2026), 250 k+ installs. Native vector + native hybrid search listed as "coming soon" |
| **VectorChord-bm25 / `pg_tokenizer.rs`** | BM25 in Postgres, TensorChord's answer to the same problem | Shipped in the vchord-suite image |

If Centinel goes Postgres, the strong configuration is **pgvector + VectorChord (or
pgvectorscale) + pg_search**, not bare pgvector + `tsvector`. That is three extensions —
which means a custom Docker image, which means you have now shipped an infrastructure
requirement, not a CLI.

### The real cost: it requires running Postgres

This is the crux. Centinel is described as a library + CLI + server + MCP that should
"install easily and run locally unattended." Postgres means: a running `postmaster`, a data
directory, a superuser to `CREATE EXTENSION`, version-pinned extension binaries compiled
against the right PG major, connection config, and an upgrade path. For a CLI that a
journalist or a civic volunteer runs on a laptop, that is the single largest install-time
failure surface in the whole system.

### Is there a credible embedded-Postgres story?

Two real options, neither of which fully rescues it:

- **[PGlite](https://pglite.dev/)** (ElectricSQL) — full Postgres compiled to WASM,
  **<3 MB gzipped**, runs in Node/Bun/Deno/browser. **pgvector is supported as a loadable
  extension** ([`@electric-sql/pglite-pgvector`](https://www.npmjs.com/package/@electric-sql/pglite-pgvector)),
  and v0.4 (Mar 2026) added PostGIS and connection multiplexing. But: it is
  **TypeScript-only** (hard language constraint), it is a single-process WASM Postgres with
  no parallel workers — the exact thing pgvector leans on for HNSW builds — and you will not
  get `pg_search`/VectorChord (native pgrx extensions) into WASM.
- **[`postgresql_embedded`](https://crates.io/crates/postgresql_embedded)** (theseus-rs,
  Rust, `#![forbid(unsafe_code)]`) — downloads real PostgreSQL binaries from
  [theseus-rs/postgresql-binaries](https://github.com/theseus-rs/postgresql-binaries) at
  runtime (or bundles them at compile time via the `bundled` feature), caches them, and
  manages the process lifecycle. It gives an SQLite-*like* developer experience but it is
  **still a real Postgres process** — and getting pgvector, let alone pgvector +
  VectorChord + pg_search, into those prebuilt archives is your problem, not the crate's.

**Verdict on §2:** pgvector is the right answer if Centinel's centre of gravity is the
*server* deployment and the CLI is a thin client to it. It is the wrong answer if the CLI
must stand alone. Given the operator simultaneously cites qmd — a zero-server SQLite tool —
as the thing they like, the honest read is that the *preference* is for pgvector but the
*requirement* is for something that runs with no daemon. See §3 and §9 for the way out
(one storage interface, two backends).

## 3. Alternatives — embedded vs server

### `sqlite-vec` — [asg017/sqlite-vec](https://github.com/asg017/sqlite-vec)

The store qmd uses. A single C extension, no dependencies, runs anywhere SQLite runs.
Backed by **Mozilla Builders**, with sponsorship from Fly.io, Turso, SQLite Cloud, Shinkai.

- **Bindings, and this is its best feature:** pip (Python), npm (Node), gem (Ruby),
  `go get`, **`cargo` (first-party Rust)**, plus Datasette, `sqlite-utils`, rqlite plugins
  and raw GitHub Releases of the loadable `.so`/`.dylib`/`.dll`.
  **This is the only candidate with genuinely first-party coverage of all three candidate
  languages via a single artifact.**
- **Vector types:** float32, **int8**, **binary** — supplied as JSON or compact binary.
- **Metadata / partition keys:** supported now (`vec0` virtual tables take metadata,
  auxiliary, and partition-key columns). Note this was *not* true at the v0.1.0 release —
  the 2024 launch post lists metadata filtering and partitioned storage as roadmap items,
  so ignore older writeups that say it can't filter.
- **Maturity:** README still says *"sqlite-vec is a pre-v1, so expect breaking changes."*
  Take that seriously.

**The critical caveat — it is brute-force only, no ANN index.** From the author's own
[v0.1.0 post](https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html):

| Corpus | Latency |
| --- | --- |
| 1 M × 128-dim, k=20 | **33 ms** (Faiss 10 ms, DuckDB 46 ms) |
| 500 k × 960-dim | **41 ms** (Faiss 50 ms) |
| 1 M × 192-dim | 192 ms |
| 1 M × 3072-dim | **8.52 s** |

The author names the practical ceiling as *"in the 100's of thousands depending on your
dimensions/quantization techniques."* ANN is committed to on the roadmap (issue #25) to
reach "low millions"/"tens of millions", but **do not plan on it existing.**

**The actionable consequence:** brute-force cost is linear in `N × d`. sqlite-vec is
viable for Centinel *if and only if* you keep dimensionality low — a 256–384-dim
Matryoshka-truncated embedding, or binary quantization (32× smaller; the author cites
"possibly only a 5–10 % loss of quality", recoverable with a rerank pass). **1 M × 128-dim
at 33 ms is a completely usable local search engine.** 1 M × 3072-dim is not. This makes
the embedding-dimension choice (§5) load-bearing in a way it simply isn't for pgvector.

### LanceDB — [lancedb/lancedb](https://github.com/lancedb/lancedb)

Embedded, in-process, columnar, built on the **Lance** open lakehouse format
([lance-format/lance](https://github.com/lance-format/lance)). Apache-2.0. This is the most
interesting candidate for Centinel specifically, because of versioning.

- **Clients:** Python, TypeScript/JavaScript, **Rust (first-party — `lancedb` and `lance`
  on crates.io; the core *is* Rust)**, plus a REST API for the cloud product. TS and Python
  are bindings over the same Rust core. **All three candidate languages covered
  first-party.**
- **Indexes:** IVF, HNSW, PQ, RQ for vectors; scalar indexes for filtering; **BM25
  full-text index built on Tantivy**. GPU-accelerated index builds available.
- **Hybrid + reranking is built in**, not bolted on — vector + BM25 fusion with pluggable
  rerankers (Cohere, ColBERT, cross-encoders) that apply to vector, FTS, *or* hybrid
  results. This is the only embedded store where hybrid+rerank is a first-class documented
  feature.
- **Storage:** local disk, or object storage (S3 and friends) directly — no server tier.
- **Versioning:** automatic **MVCC with ACID transactions, zero-copy versioning, time
  travel, tags, and branches**, with no extra infrastructure. Every write creates a version;
  `table.checkout(version)` reads the table as of that version. Vector indexes are versioned
  along with the data.

LanceDB's own docs use a **Federal Register** corpus as the worked
[time-travel RAG example](https://docs.lancedb.com/tutorials/agents/time-travel-rag) —
i.e. a versioned government-document corpus. Not a coincidence worth ignoring.

Its scale story sits between sqlite-vec and Postgres: LanceDB's own
[WikiSearch writeup](https://www.lancedb.com/blog/feature-full-text-search) demonstrates
native full-text search over **41 M Wikipedia documents**, which is an order of magnitude
past anything Centinel needs.

### Server-based stores

| Store | License | Written in | First-party Rust client? | Embedded mode |
| --- | --- | --- | --- | --- |
| **[Qdrant](https://github.com/qdrant/qdrant)** | Apache-2.0 | **Rust** | **Yes** (`qdrant-client` crate, gRPC) | No. The Python client's "local mode" is a *reimplementation*, not the engine |
| **[Chroma](https://github.com/chroma-core/chroma)** | Apache-2.0 | Python + Rust core | Community only | Yes — persistent local mode; easiest server-store to run locally |
| **[Weaviate](https://github.com/weaviate/weaviate)** | BSD-3-Clause | Go | No | No |
| **[Milvus](https://github.com/milvus-io/milvus)** | Apache-2.0 | Go + C++ | No | Milvus Lite — **Python only** |

Qdrant is the strongest of these for a Rust codebase (excellent single-node performance,
HNSW + SQ/PQ/BQ quantization + sparse vectors for hybrid), and it is one binary rather than
a cluster. Milvus is the wrong shape — built for billion-scale distributed deployment with
the operational weight to match. Weaviate and Milvus both effectively demote Rust to
"talks HTTP to a Go service."

**None of these solve the install problem.** They trade "you must run Postgres" for "you
must run Qdrant." If a server is acceptable at all, **Postgres is the better server**,
because Centinel wants relational metadata (crawl runs, URLs, versions, provenance,
extraction status) anyway, and pgvector keeps that in one place instead of two.

### `tantivy` — [quickwit-oss/tantivy](https://github.com/quickwit-oss/tantivy)

MIT, Rust, a Lucene-style full-text engine with real **BM25**. Not a vector store, but
structurally important:

- It is the BM25 engine **inside both LanceDB and ParadeDB's `pg_search`** — choosing
  either means you are running tantivy whether you name it or not.
- Bindings: `tantivy-py` for Python is maintained. **There is no production-grade Node
  binding** — a genuine asymmetry.
- If Centinel goes Rust and wants full control: `tantivy` + `sqlite-vec` or `lancedb` +
  RRF is a fully-owned, server-free hybrid stack.

**In TypeScript, the practical equivalent of tantivy is SQLite FTS5** — which is what qmd
uses, which is a real BM25 implementation, and which costs nothing extra because you are
already linking SQLite.

## 4. The versioned-corpus problem

**This is genuinely under-served, and it is the part of Centinel that no off-the-shelf
vector-DB tutorial addresses.** Almost all vector-DB documentation assumes a mutable
"current state" corpus: you upsert, you delete, the old vector is gone. Centinel's premise
is the opposite — every version is retained forever, and the diff between versions *is the
product*. ("What changed on the permitting page between March and June?" is a better
question than "what does the permitting page say?")

### The two failure modes, named

The [VersionRAG paper](https://arxiv.org/html/2510.08109v1) (Oct 2025) names them cleanly,
and both apply directly:

1. **Version conflation** — a naive index returns semantically similar chunks from
   *multiple* versions with no temporal discrimination, so the answer mixes three years of
   contradictory policy into one response. Measured: naive RAG scores **55 %** on
   version-specific queries.
2. **Implicit change tracking** — asking "when did this requirement disappear?" is
   unanswerable by similarity search, because *the absence of text has no embedding*.
   Measured: naive RAG **0 %**, GraphRAG **10 %** on implicit-change queries.

VersionRAG's own results on their VersionQA benchmark (100 QA pairs over 34 technical docs,
710+ pages, DeepSeek-R1 70B):

| Category | VersionRAG | Naive RAG | GraphRAG |
| --- | --- | --- | --- |
| Overall | **90 %** | 58 % | 64 % |
| Content retrieval | 93.3 % | 76.6 % | 70.0 % |
| Version-specific | 100 % | 55 % | 100 % |
| Version listing | 100 % | 35 % | 80 % |
| Change retrieval | 70 % | 25 % | 30 % |
| **Implicit changes** | **60 %** | **0 %** | 10 % |

Indexing cost: 186 K tokens / $0.17 / 25 min, vs GraphRAG's 2,970 K tokens / $6.67 /
5 h 12 min — a **97 % token reduction**, because version relationships are encoded as
*graph edges* rather than extracted by an LLM. The architecture is a 5-level graph:
category → document → version (with temporal-sequence edges) → content chunk → **change
node** (explicit changes from changelogs, implicit changes from `DeepDiff` line-level
comparison + LLM interpretation). Query intent is classified and routed: content queries →
vector search *with a version filter*; version queries → graph traversal; change queries →
search over the change nodes.

**The transferable lesson: model "change" as an indexed first-class object, not something
you hope similarity search will surface.** For Centinel that means a `diff` record between
consecutive versions of a page, with its own embedded summary. That is what makes "what
changed" answerable at all.

### Index all versions, or only current?

The honest recommendation is **neither, exactly — index all *distinct chunks*, once, and
let versions reference them.**

The whole problem dissolves if chunk identity is `hash(chunk_text)`:

- A city page re-crawled monthly is ~95 % identical each time (nav, footer, boilerplate,
  unchanged body). **Only genuinely new chunks need embedding.**
- Embedding cost becomes proportional to *actual change*, not to `pages × versions`.
- Storage: one vector per distinct chunk, plus a cheap `(version_id, chunk_hash, ordinal)`
  join table. That table is tiny relative to vectors.
- This is exactly what qmd already does — docid = first 6 chars of the content hash, and
  `qmd update` re-embeds only changed content (§1). Centinel's file tree is already
  hash-addressed, so the primitive is free.

Then layer a **two-tier retrieval default**:

- **Hot tier: current version only.** Almost all queries mean "what does the government say
  *now*". Default the search to `is_current = true`. This keeps the working set small enough
  that even sqlite-vec's brute force is fast.
- **Cold tier: all historical versions**, same vectors (they're shared!), reachable via an
  explicit `--as-of <date>` / `--all-versions` flag.

This dual-tier split is precisely what
[LiveVectorLake](https://arxiv.org/html/2601.05270) proposes — hot tier with HNSW for
sub-100 ms latency, cold tier in columnar storage optimized for retention cost — and it is
the right shape here. The difference is that with content-hash chunk identity you don't
duplicate the vectors between tiers; the tier is a *filter*, not a copy.

### Stores with native versioning

**LanceDB is the only serious candidate with this built in.** Automatic MVCC, ACID
transactions, zero-copy versioning, **time travel, tags, and branches**, no extra
infrastructure. `table.checkout(version)` reads the table as of a point in time, and vector
indexes are versioned alongside the data (§3).

Two honest caveats before treating that as the answer:

1. **LanceDB versions the *table*, not the *documents*.** Its time travel answers "what did
   my index look like at write #47", which is a database-history question. Centinel needs
   "what did *this URL* look like on 2026-03-14", which is a domain question. These
   coincide only if you discipline your writes so that one crawl run = one LanceDB version.
   That is doable and appealing (a crawl run becomes an atomic, taggable, rollback-able
   commit — tag it `crawl-2026-03-14`), but it is a design commitment, not a freebie.
2. Under the content-hash scheme above, **you probably want document versions as ordinary
   rows anyway** — because you want to query across versions (`WHERE first_seen < X AND
   last_seen > X`), not check out one at a time. Table-level time travel is orthogonal, and
   most valuable as *crash safety and reproducibility for the index build*, which is
   genuinely useful: a bad crawl can be rolled back.

Nothing else on the list has native versioning. pgvector/Postgres gives you nothing
automatic, but Postgres is entirely comfortable modeling versions as rows with validity
ranges (`tstzrange` + GiST exclusion constraints is a well-worn pattern), and that is
arguably *more* honest than leaning on a database-history feature.

### Cost profile of recrawl

Rough arithmetic, worth doing before choosing anything:

- 50 k pages × ~8 chunks = 400 k chunks per full snapshot.
- Recrawl monthly. **Naive (embed everything, every time): 4.8 M embeddings/year, growing
  linearly forever.** After 3 years: ~14 M vectors — past sqlite-vec's brute-force ceiling
  and into pgvector's uncomfortable zone.
- **Content-hash dedup, assuming 5 % of chunks change per crawl: 400 k initial + ~20 k/month
  = ~640 k after 3 years.** A 20× reduction, and it stays inside sqlite-vec's envelope.

**This single decision — hash-keyed chunk dedup — is worth more than the choice of vector
store.** It is what makes an embedded store viable at all, and it is entirely
language-independent.

PDFs behave even better: a PDF is immutable once published; a new agenda packet is a new
document, not a new version, so there is no dedup opportunity but also no version
explosion. Transcripts likewise: a meeting happens once.

### Other prior art

- **[GovScape](https://arxiv.org/abs/2511.11010)** (Nov 2025, `govscape.net`, open source) —
  the closest existing system to Centinel. **10,015,993 federal government PDFs /
  70,958,487 pages** from the **2020 End of Term crawl**. Stack: **BGE** embeddings for
  semantic text search, **CLIP** for visual search ("show me redacted documents", "pie
  charts"), **olmOCR** for scanned pages, **FAISS** for large-scale vector indexing,
  **LanceDB**, **Apache Lucene** and **SQLite FTS5** for full text. **Total preprocessing
  compute: ~$1,500 — i.e. ~47,000 PDF pages per dollar.** Read this paper before building.
  Notably it does *not* address cross-crawl versioning, which is precisely Centinel's
  differentiator.
- **[Building and Querying Semantic Layers for Web Archives](https://arxiv.org/pdf/1810.10455)** —
  earlier work on exactly the archive-plus-semantics problem.
- **["A time machine for text search"](https://dl.acm.org/doi/10.1145/1277741.1277831)**
  (SIGIR 2007) — the classical time-travel inverted index. Predates embeddings entirely, but
  the index partitioning ideas hold up.
- The 2007 SIGIR framing still reads as current: *text search over temporally versioned
  collections such as web archives has received little research attention.* Nearly two
  decades later, **the vector-search version of this problem is still open.** Centinel is
  in genuinely under-served territory here — which is both the risk and the opportunity.

## 5. Embedding models

### The BM25 question — checked against primary sources, and the answer is *no, but*

The prior run's flagged finding was **"BM25 tops some long-context retrieval benchmarks."**
Checked directly, **that is not literally true**, but the thing underneath it is true and
more useful.

What the primary sources actually say:

- **[BRIGHT](https://arxiv.org/abs/2407.12883)** (reasoning-intensive retrieval) abstract,
  verbatim: *"The leading model on the MTEB leaderboard … SFR-Embedding-Mistral …, which
  achieves a score of 59.0 nDCG@10, produces a score of nDCG@10 of 18.3 on BRIGHT."*
  **A 40-point collapse from BEIR to BRIGHT.**
- **[Lighting the Way for BRIGHT](https://arxiv.org/html/2509.02558)** (reproducible
  Anserini/Pyserini baselines; they replicate BRIGHT's original BM25 numbers to within
  0.3 pp):

  | System | BRIGHT nDCG@10 |
  | --- | --- |
  | BM25 (bag of words) | 13.7 |
  | **BM25Q** (query-side BM25) | **14.8** |
  | BGE-large-en-v1.5 | 13.8 |
  | SPLADE-v3 | 15.6 |
  | Diver-Retriever-4B (reasoning-tuned) | 29.1 |
  | Reason-Embed-4B (reasoning-tuned) | 36.9 |

  So BM25 does **not** top the leaderboard — reasoning-tuned dense retrievers beat it by
  2.5×. But **BM25 ties or beats a strong general-purpose embedding model** (13.7 vs BGE's
  13.8), which is the honest, defensible version of the claim.
- On **BEIR/MTEB proper**, dense retrieval now clearly wins; the BM25 baseline is
  ~42 nDCG@10 and current leaders are ~60+.
- On **long-context** specifically: [LongEmbed](https://arxiv.org/abs/2404.12096) says only
  *"huge room for improvement"* in embedding models and makes **no BM25 comparison at all**.
  [LoCoV1](https://arxiv.org/html/2402.07440v2) is won by specialized long-context encoders
  (M2-BERT), not BM25. The "BM25 wins on long context" claim does not trace to a primary
  source.

**What this means for Centinel's default, concretely:**

1. **Do not ship pure vector search as the default.** BM25 is free (FTS5/tantivy),
   instant, has no model dependency, and on hard out-of-domain corpora is within noise of
   a good general embedding model. Government text is *full* of exact tokens users search
   for — ordinance numbers, parcel IDs, statute cites, proper names, dollar amounts — where
   BM25 is not merely competitive but categorically better, because embeddings blur exact
   strings.
2. **Hybrid + RRF is the correct default**, which is exactly what qmd does.
3. **The single largest measured gain in that BRIGHT table is not the retriever — it's the
   reranker.** See §6.

### Local models — what can actually run in-process, per language

| Runtime | Rust | Python | TypeScript | Notes |
| --- | --- | --- | --- | --- |
| **[`fastembed`](https://crates.io/crates/fastembed)** | **Yes, first-party** | **Yes** ([`fastembed`](https://qdrant.github.io/fastembed/)) | **Yes** ([`fastembed-js`](https://github.com/anush008/fastembed-js)) | Qdrant's library. ONNX Runtime under the hood, CPU-first, minimal deps (explicitly targets serverless). **The only embedding runtime with genuine first-party coverage of all three languages.** |
| **ONNX Runtime** | `ort` crate | `onnxruntime` | `onnxruntime-node` / `onnxruntime-web` | The substrate under fastembed and transformers.js |
| **`llama.cpp` (GGUF)** | `llama-cpp-rs` bindings | `llama-cpp-python` | **`node-llama-cpp`** | What **qmd** uses. Gives you embeddings *and* rerankers *and* generation from one runtime and one model format |
| **sentence-transformers** | — | **Yes, canonical** | — | Python-only. The reference implementation everything else chases |
| **Candle** (HF) | **Yes, native Rust** | — | — | Pure-Rust inference; more assembly required than fastembed |
| **[`model2vec`](https://github.com/MinishLab/model2vec)** | Yes (`model2vec-rs`) | Yes | — | **Static** distilled embeddings — no transformer at query time, just a lookup + pool. Orders of magnitude faster, meaningfully lower quality. Excellent for a "no model download" fallback tier |
| **`transformers.js`** | — | — | **Yes** | HF's JS port over ONNX Runtime; runs in Node and the browser |

**None of these constrain the language.** All three candidates can embed in-process with no
Python subprocess and no HTTP call. That is a genuinely settled question in 2026 and should
not be used as an argument for any particular language.

### Concrete model candidates

| Model | Dims (MRL) | Ctx | Size | Notes |
| --- | --- | --- | --- | --- |
| **[EmbeddingGemma-300M](https://huggingface.co/google/embeddinggemma-300m)** | **768 → 512 → 256 → 128** | 2,048 | 300 M params (~300 MB @ Q8) | qmd's default. MTEB v2: **69.67 English**, 61.15 multilingual. QAT Q4_0/Q8_0 checkpoints. **Gemma license** (must accept Google terms — check this before redistributing). Prompt prefixes are mandatory: docs `"title: {title\|'none'} \| text: {content}"`, queries `"task: search result \| query: {content}"` |
| **[Qwen3-Embedding-0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B)** | 1024, MRL | **32 K** | 0.6 B | Apache-2.0. 32 K context matters for transcripts |
| **[Qwen3-Embedding-8B](https://huggingface.co/Qwen/Qwen3-Embedding-8B)** | 4096, MRL | 32 K | 8 B | #1 MTEB multilingual at release (70.58). **4096 dims exceeds pgvector's HNSW `vector` limit — needs `halfvec`** |
| **BGE family** (BAAI) | 384–1024 | 512–8 K | small | What **GovScape used for 10 M government PDFs** — the closest precedent to Centinel's corpus |
| **`model2vec` statics** | 256–512 | n/a | ~30 MB | Fallback tier; no GPU, no ONNX, near-instant |

**The 2,048-token context on EmbeddingGemma is a real constraint for transcripts** (§7) and
argues for Qwen3-Embedding-0.6B's 32 K if you want to embed long spans whole.

### API models — cost per 1M tokens

| Provider | Model | $/1M tokens |
| --- | --- | --- |
| OpenAI | `text-embedding-3-small` (1536, MRL-truncatable) | **$0.02** |
| OpenAI | `text-embedding-3-large` (3072, MRL-truncatable) | **$0.13** |
| OpenAI | `text-embedding-ada-002` (1536, legacy) | $0.10 |
| [Voyage](https://docs.voyageai.com/docs/pricing) | `voyage-4-lite` | **$0.02** |
| Voyage | `voyage-4` | $0.06 |
| Voyage | `voyage-4-large` / `voyage-context-4` | $0.12 |
| Voyage | `voyage-law-2` | $0.12 |
| Voyage | `rerank-2.5-lite` / `rerank-2.5` | $0.02 / $0.05 |
| [Cohere](https://cohere.com/pricing) | Embed 4 / Rerank 3.5 / Rerank 4 | Per-token API rates not on the public page; the visible pricing is hourly **Model Vault** deployment ($4–10/hr). Rerank billed in "search units" = 1 query × up to 100 docs, docs >500 tokens auto-split |

**Sanity check on API cost.** 50 k pages × ~4 k tokens = **200 M tokens**. At
`text-embedding-3-small` / `voyage-4-lite` that is **$4 for a full corpus embed**. At
`text-embedding-3-large`, $26. **Embedding is not the expensive part** — even re-embedding
the whole corpus from scratch is a rounding error next to crawling and OCR. GovScape's
independent anchor: **10 M PDFs / 71 M pages preprocessed for ~$1,500 total compute, i.e.
~47,000 PDF pages per dollar** ([GovScape](https://arxiv.org/abs/2511.11010)).

**This materially changes the architecture conversation.** If a full re-embed costs single-
digit dollars and a few hours, then "the vector index is a disposable derived artifact" is
not aspirational — it is simply true, and you should design for wholesale rebuilds rather
than clever incremental correctness. (Local embedding is even cheaper in dollars, just
slower in wall-clock.)

### Dimensionality, storage, and Matryoshka

Storage per vector in pgvector: `4·d + 8` bytes fp32, `2·d + 8` fp16. Per 1 M vectors:

| Dims | fp32 | fp16 (`halfvec`) | binary |
| --- | --- | --- | --- |
| 128 | 0.5 GB | 0.26 GB | 16 MB |
| 384 | 1.5 GB | 0.77 GB | 48 MB |
| 768 | 3.1 GB | 1.5 GB | 96 MB |
| 1536 | 6.1 GB | 3.1 GB | 192 MB |
| 3072 | 12.3 GB | 6.1 GB | 384 MB |

**Matryoshka Representation Learning (MRL) is the most useful practical lever here.** MRL
models are trained so that a *prefix* of the vector is itself a valid embedding — you can
truncate 768 → 256 and keep most of the quality, with no re-embedding. EmbeddingGemma
(768→512→256→128), Qwen3-Embedding, OpenAI `text-embedding-3-*`, and Voyage all support it.

Combined with sqlite-vec's brute-force cost model (§3), this is the whole game: **embed
once at full width, store truncated (256-dim) for the fast first-stage scan, keep the full
vector only if you need it.** 1 M × 256-dim brute force is well under 100 ms.

Binary quantization is the more aggressive version — 32× smaller, "possibly only a 5–10 %
loss of quality" per sqlite-vec's author, and that loss is largely recoverable by
over-fetching and reranking (§6). qmd's own reranker pass does exactly this job.

### Model churn — re-embedding when a better model ships

Bluntly: **nothing meaningfully mitigates this, and you should stop trying to.**

- Embedding spaces are model-specific. There is no "convert vectors from model A to model
  B." Research on cross-model vector alignment / adapters exists but nothing production-safe.
- MRL helps you shrink *within* a model, not migrate *between* models.
- Partial mitigations that do work:
  - **Store the chunk text alongside the vector** (or at minimum the exact byte offsets into
    the source file) so a re-embed never requires re-crawling or re-extracting. This is the
    single highest-value defensive decision, and it is free.
  - **Record the model id + dimension + prompt-prefix template per vector row.** Then you can
    run two models side by side during a migration and A/B them, rather than doing a
    big-bang cutover.
  - **Version the index directory** (`index/v1-embeddinggemma-768/`,
    `index/v2-qwen3-1024/`) and swap atomically by symlink. LanceDB's branches/tags give
    you this natively; on SQLite it's a file rename.
- And the actual answer: **at $4–26 per full corpus re-embed (above), model churn is a
  compute-cost non-problem.** The cost is wall-clock and operational risk, not money. Design
  the rebuild path to be a first-class, tested, single-command operation (§8) and model
  churn stops being scary.

## 6. Reranking

**Reranking is the highest-leverage component in this whole document, and the evidence for
that is unusually clean.**

### The measured gain

From [Lighting the Way for BRIGHT](https://arxiv.org/html/2509.02558), listwise LLM
reranking (gpt-oss-120b) applied on top of different first-stage retrievers:

| First stage | nDCG@10 before | after rerank | gain |
| --- | --- | --- | --- |
| **BM25Q + NAF fusion** | 14.8 | **33.4** | **+18.6** |
| Diver-Retriever-4B | 29.1 | 39.1 | +10.0 |
| Reason-Embed-4B | 36.9 | 40.3 | +3.4 |

Read that table carefully. **Reranking on top of plain BM25 (33.4) beats an
expensively-trained reasoning-tuned dense retriever without reranking (29.1).** And the
better your first stage, the *smaller* the reranking gain — the reranker is substituting
for retriever quality, and it is cheaper to acquire.

For Centinel this is the architectural punchline: **a cheap lexical + cheap-embedding first
stage that over-fetches, followed by a good reranker, beats an expensive retriever alone.**
It also means you can afford aggressive quantization and MRL truncation in the first stage
(§5) because the reranker cleans up the recall you traded away. That is precisely why qmd
pairs a 300 M embedder with a 0.6 B cross-encoder.

### Local cross-encoder rerankers

| Model | Params | License | Score | Notes |
| --- | --- | --- | --- | --- |
| **[Qwen3-Reranker-0.6B](https://huggingface.co/Qwen/Qwen3-Reranker-0.6B)** | 0.6 B | **Apache-2.0** | MTEB-R **65.80**, CMTEB-R 71.31 | **What qmd uses** (Q8_0 GGUF). **32 K context**, 100+ languages, instruction-aware. Invocation: `"<Instruct>: {instruction}\n<Query>: {query}\n<Document>: {doc}"` |
| Qwen3-Reranker-4B | 4 B | Apache-2.0 | MTEB-R **69.76** | Meaningfully better, meaningfully slower |
| Qwen3-Reranker-8B | 8 B | Apache-2.0 | 69.02 multilingual | |
| **[BAAI/bge-reranker-v2-m3](https://huggingface.co/BAAI/bge-reranker-v2-m3)** | 568 M | **Apache-2.0** | MTEB-R 57.03, CMTEB-R 72.16, MIRACL ~69.32 | The long-standing default. Strong multilingual, weaker English than Qwen3 |
| **mxbai-rerank-base-v2 / large-v2** | 0.5 B / 1.5 B | **Apache-2.0** | ~0.911 nDCG@10 on a GPT-4o-labelled eval | Solid Apache-2.0 alternative |
| **jina-reranker-v2-base-multilingual** | 278 M | **CC-BY-NC-4.0** | ~0.907 on the same eval; 57.06 BEIR | **Non-commercial license — production use requires the Jina API or a commercial deal.** Rules it out for a redistributable OSS tool |
| jina-reranker-v3 | — | check | **61.94 BEIR** (vs v2's 57.06), 66.50 MIRACL | Strong, but same licensing question |

**License note that matters for Centinel:** if the tool ships models or auto-downloads them,
Jina's CC-BY-NC is disqualifying and EmbeddingGemma's Gemma license needs reading. **Qwen3
(embed + rerank) is Apache-2.0 across the board** — the cleanest licensing story, and it
also gives you 32 K context on both halves.

### API rerankers

| Provider | Model | Price |
| --- | --- | --- |
| Voyage | `rerank-2.5-lite` | **$0.02 / 1M tokens** |
| Voyage | `rerank-2.5` | **$0.05 / 1M tokens** |
| Cohere | Rerank 3.5 / Rerank 4 Fast / Rerank 4 Pro | Billed in "search units" (1 query × up to 100 docs; docs >500 tokens auto-split). Public page currently shows hourly Model Vault pricing ($5–10/hr) |

Rerank API cost is negligible per query but scales with `queries × candidates`, not corpus
size — the opposite cost curve from embedding. A local reranker is the better default for a
tool that runs unattended and offline.

### What can run a reranker in-process, per language

| Runtime | Rust | Python | TypeScript |
| --- | --- | --- | --- |
| **llama.cpp / GGUF** | `llama-cpp-rs` | `llama-cpp-python` | **`node-llama-cpp`** — qmd's path |
| **ONNX Runtime** | `ort` | `onnxruntime` | `onnxruntime-node` |
| **fastembed** (has rerank support) | **Yes** | **Yes** | Yes |
| sentence-transformers `CrossEncoder` | — | Canonical | — |
| Candle | Native Rust | — | — |
| transformers.js | — | — | Yes |

**Reranking does not constrain the language either.** All three can run a 0.6 B
cross-encoder in-process on CPU. The practical difference is throughput: a 0.6 B
cross-encoder scoring 30–40 candidates is a few hundred milliseconds to a couple of
seconds on CPU, which is why qmd makes it an opt-in command (`qmd query`) rather than the
default (`qmd search`). **Copy that ergonomic decision.**

### Recommended pipeline shape

```
query
  ├─ BM25 (FTS5 / tantivy)        → top 100     [instant, no model]
  └─ vector (truncated/quantized) → top 100     [fast, small model]
        └─ RRF fuse (k=60)        → top 30–40
              └─ cross-encoder rerank           [opt-in, ~0.5–2 s CPU]
                    └─ top 10
```

Optional fourth stage, cheap and high-value for a `.gov` corpus: **metadata boost** —
prefer the current version over historical, prefer official domains, prefer recency for
"what is the rule now" queries. Cheap to implement, and it addresses version conflation
(§4) directly.

## 7. Chunking

### Library landscape, with honest language coverage

| Library | Rust | Python | TS | What it gives you |
| --- | --- | --- | --- | --- |
| **[`text-splitter`](https://github.com/benbrandt/text-splitter)** (a.k.a. `semantic-text-splitter` on PyPI) | **Native Rust, MIT** | **Yes** (PyO3 bindings) | **No** | `TextSplitter`, **`MarkdownSplitter`** (CommonMark-aware), `CodeSplitter` (tree-sitter). Splits at a *hierarchy* of semantic boundaries and falls down the hierarchy only as needed |
| **[Chonkie](https://github.com/chonkie-inc/chonkie)** | **[`chonkie-rs`](https://crates.io/crates/chonkie)** | **Canonical** | **[`chonkie-ts`](https://github.com/chonkie-inc/chonkie-ts)** (not at feature parity) | Token / Word / Sentence / **Recursive** / **Semantic** / **SDPM** / **Late** chunkers, plus markdown "recipes" and markdown-table handling. Rust-backed tokenizers |
| **[Docling](https://docling-project.github.io/docling/concepts/chunking/)** `HybridChunker` | No | **Python only** | No | Document-hierarchy chunking with **tokenization-aware refinement** on top. Built for converted PDFs — knows about headings, tables, page structure |
| **LangChain splitters** | No | Yes | Yes | `RecursiveCharacterTextSplitter`, `MarkdownHeaderTextSplitter`. Ubiquitous, mediocre, heavy dependency |
| **qmd's own** | — | — | its own TS | Scored break points (headings 50–100, code fences 80, blank lines 20), ~900-token target, 15 % overlap, 200-token search window (§1) |

**Language note:** `text-splitter` gives Rust+Python but **not** TypeScript. Chonkie covers
all three but the TS port lags. **In TypeScript, qmd's scored-boundary algorithm is ~150
lines and worth just writing** — it is the least exotic part of this whole system, and
qmd is MIT so you can read it.

Chunking is **not** a language-constraining decision. It is, however, the decision most
likely to determine retrieval quality, and it is the cheapest to iterate on.

### Shape 1 — markdown pages (easy)

Heading-aware structural chunking, and both `MarkdownSplitter` and qmd's scored-boundary
approach do it well. Specifics that matter for `.gov` pages:

- **Prepend the heading path to every chunk** (`Planning > Permits > Fees > chunk text`).
  Cheap, and it fixes the classic failure where a chunk about "the fee is $250" has no
  indication which permit it refers to. EmbeddingGemma's `title:` prefix slot exists
  exactly for this.
- **Strip nav/footer/boilerplate before chunking**, not after. Every page on a city site
  shares a 500-word chrome block; embedding it once per page poisons similarity scores and
  wastes most of your vectors. This is also what makes content-hash dedup (§4) effective —
  identical boilerplate hashes identically and collapses to one vector.
- Target ~900 tokens with ~15 % overlap is a reasonable, evidence-free-but-widely-used
  default; qmd landed there and it is fine.

### Shape 2 — PDF-extracted text (medium)

The 400-page agenda packet is the adversarial case, and it is a real one for county/city
government.

- **Docling's `HybridChunker` is the strongest named tool** here, and it is **Python-only**.
  If Centinel goes Rust or TS, you are re-implementing or shelling out. This is one of the
  few places where Python has a genuine, hard-to-replicate library advantage.
- **Preserve page numbers as chunk metadata.** Non-negotiable for a public-accountability
  tool — a citation that says "page 213 of the June 4 agenda packet" is verifiable; one
  that says "somewhere in this 400-page PDF" is not.
- **Tables break naive chunkers.** A table split mid-row is worse than useless. Chonkie has
  explicit markdown-table handling; Docling models tables structurally. Consider extracting
  tables separately and embedding a serialized form (or an LLM-generated caption) as its
  own chunk type.
- **Agenda packets are really N documents concatenated.** Detect the internal document
  boundaries (bookmark tree, "ITEM 4.B", repeated cover-page patterns) and treat them as
  top-level splits before doing anything else. Getting this right is worth more than any
  chunker choice.

### Shape 3 — meeting transcripts (hard, and the thinnest literature)

**This is the least-solved of the three, and the research on it is genuinely sparse.** A
three-hour council meeting is ~30,000 words of low-density speech with no headings, and
depending on the ASR source, possibly **no punctuation and no speaker labels** (raw YouTube
auto-captions are exactly this).

What is actually known:

- **Turn-based chunking is the established primitive where speaker labels exist.** The
  meeting-summarization literature is explicit that linear segmentation interrupting
  speaker turns hurts, and proposes maximizing tokens per chunk **subject to never
  splitting a speaker turn**
  ([Action-Item-Driven Summarization of Long Meeting Transcripts](https://arxiv.org/pdf/2312.17581)).
- **Semantic / topic-shift chunking is the fallback where they don't.** Embed each
  turn or sentence window, compute cosine similarity with the previous, start a new chunk
  when similarity drops below a threshold. This is a direct descendant of **TextTiling**
  (lexical-cohesion subtopic boundaries). Chonkie's `SemanticChunker` and `SDPMChunker`
  implement this family.
- **Hierarchical segmentation helps RAG measurably** — see
  [Enhancing RAG with Hierarchical Text Segmentation Chunking](https://arxiv.org/pdf/2507.09935).
- **Diarization-aware pipelines** chunk long-form audio into fixed ~30 s segments and then
  group them under constraints on max duration and speaker-segment count
  ([Diarization-Aware Multi-Speaker ASR via LLMs](https://arxiv.org/pdf/2506.05796)).

Practical recommendations specific to Centinel's case:

1. **Get speaker labels and timestamps if you possibly can.** YouTube auto-captions carry
   per-segment timestamps even without diarization. **Timestamps are the transcript's
   equivalent of PDF page numbers** — they turn a retrieval hit into a
   `youtube.com/watch?v=X&t=4271s` citation, which is the entire value proposition for a
   civic-accountability tool. Store them per chunk unconditionally.
2. **Chunk on agenda items, not on tokens, when you can.** A council meeting has a
   published agenda. Aligning transcript spans to agenda items — via timestamp heuristics
   or lexical matching against the agenda PDF — gives you real structure where the
   transcript has none, and *cross-links two of your three corpora*. This is the highest-
   value transcript idea in this document, and it is available to Centinel precisely because
   it also harvests the agenda PDFs.
3. **Restore punctuation/sentence boundaries first** if the ASR output lacks them. A
   punctuation-restoration pass before chunking makes every downstream splitter work; without
   it, "sentence boundary" splitting degenerates to arbitrary splitting.
4. **Expect low information density and compensate with context, not smaller chunks.**
   Procedural filler ("motion carries", "all in favor") dominates. Smaller chunks make this
   worse (more pure-filler chunks). Prefer larger chunks (1,000–1,500 tokens) plus a
   generated per-chunk summary/topic line embedded alongside — or **late chunking** (embed
   the long document once with a long-context model, then pool per-chunk from the
   contextualized token embeddings), available as Chonkie's `LateChunker`. Note this needs a
   long-context embedder: **EmbeddingGemma's 2 K limit is too short; Qwen3-Embedding's 32 K
   is not** (§5).
5. **BM25 earns its keep hardest here.** Names, motions, addresses, ordinance numbers,
   dollar figures — the things people actually search meeting records for — are exact
   tokens. Do not ship transcript search without lexical retrieval.

**Honest summary: for transcripts, there is no library you can just adopt.** The three
shapes want three different pipelines, and the transcript pipeline is bespoke work.

## 8. Index-rebuild story

### Split the derived artifact in two — this is the key move

"The index is a derived, rebuildable artifact" is true but too coarse, because the two
things being rebuilt have wildly different costs:

| Tier | Cost to rebuild | Keyed by | Portable across backends? |
| --- | --- | --- | --- |
| **A. Embedding cache** — `chunk_hash → vector` | **Expensive** (model inference over the whole corpus) | content hash | **Yes** — it's just bytes |
| **B. The search index** — ANN graph, BM25 postings, metadata | **Cheap** (minutes) | — | No, backend-specific |

**Tier B is genuinely disposable. Tier A should be treated as durable, and it is the thing
worth backing up.** Concretely: store the embedding cache as its own artifact (a SQLite
table, or a Parquet/Lance file) keyed by `(chunk_hash, model_id, dims)`, sitting *beside*
the static files rather than inside any vector store. Then:

- Swapping vector backends (sqlite-vec → pgvector, or the reverse) is a re-import, not a
  re-embed. **This is what lets you defer the storage decision.**
- A corrupted index is a `rm -rf` and a few minutes, never a re-embed.
- Adding a new model is additive — a second cache namespace, A/B-able (§5).
- The cache is the natural unit to publish. A public "Centinel embeddings for
  cityofX.gov" artifact lets others build on the work without re-crawling.

### Rebuild cost per backend (Tier B only, assuming the embedding cache is warm)

| Store | Index build | Notes |
| --- | --- | --- |
| **sqlite-vec** | **Effectively zero** — brute force means *there is no index to build*. Rebuild = insert rows | The genuinely disposable extreme. Rebuild is I/O-bound, minutes for millions of rows |
| **SQLite FTS5** | Fast; `INSERT INTO fts(fts) VALUES('rebuild')` is a supported one-liner | |
| **LanceDB** | IVF/HNSW/PQ build; GPU-accelerated builds available. **Plus: you don't have to rebuild in place** — write a new version and `checkout`/tag it, with rollback for free | Best-in-class rebuild *ergonomics* |
| **tantivy** | Fast segment-based indexing; merge policy handles compaction | |
| **pgvector IVFFlat** | Cheap, but **must be built after the data is loaded** — so the rebuild order is fixed: `COPY` then `CREATE INDEX` | |
| **pgvector HNSW** | **The expensive one.** `maintenance_work_mem`-bound; degrades sharply once the graph exceeds RAM. Raise `max_parallel_maintenance_workers` (30–50 % faster on multi-core). Budget hours, not minutes, at 5–10 M vectors | |
| **Qdrant** | HNSW is built incrementally per segment on upsert; "rebuild" = re-upsert the collection | No separate build phase, but no cheap atomic swap either |

### The pattern to implement

```
centinel index rebuild
  1. walk the static file tree (source of truth)
  2. chunk → chunk_hash
  3. look up chunk_hash in the embedding cache
       hit  → reuse             (the overwhelming majority)
       miss → embed, write back to cache
  4. write vectors + metadata into a NEW index directory
  5. atomically swap (symlink rename / LanceDB tag / Postgres schema rename)
  6. keep the previous index for one generation, then GC
```

Steps 4–6 give you a rebuild that never takes search offline and is always rollback-able.
This is **identical in every candidate language** and is the single most important thing to
get right structurally.

### Reality check on "how bad is a from-cold rebuild?"

Worst case — no cache, full re-embed of 50 k pages / 200 M tokens:

- **API embedding:** ~$4 (`text-embedding-3-small`, `voyage-4-lite`) to ~$26 (`3-large`).
  Wall-clock hours, parallelizable, and batch APIs are cheaper still.
- **Local embedding:** $0, but a 300 M-param model on CPU is roughly single-digit hours
  to a day for a corpus this size. GPU or a `model2vec` static model collapses that.
- **Independent anchor:** GovScape preprocessed **10 M PDFs / 71 M pages for ~$1,500**
  (~47 k pages/dollar) ([arXiv 2511.11010](https://arxiv.org/abs/2511.11010)) — and that
  figure includes **olmOCR**, which is far more expensive than embedding.

**Conclusion: the index genuinely is disposable, and even the embedding cache is
affordable to lose.** The stated architecture holds up under scrutiny. The thing you must
never lose is the static file tree — which is exactly the thing the design already treats
as source of truth.

## 9. What this means for the language decision

**Not picking.** Here is what the search layer actually constrains and what it doesn't.

### Language-independent (do not let these influence the choice)

- **Vector storage.** sqlite-vec, LanceDB, pgvector, and Qdrant all have first-party or
  effectively-first-party access from Rust, Python, and TypeScript. sqlite-vec is a C
  extension every language can load; LanceDB is a Rust core with Python/TS bindings;
  Postgres is a wire protocol.
- **In-process embedding.** `fastembed` covers all three first-party. llama.cpp has
  maintained bindings for all three (`node-llama-cpp` is what qmd uses and is solid).
  ONNX Runtime likewise. **This was a real constraint two years ago and no longer is.**
- **In-process reranking.** Same story — a 0.6 B cross-encoder runs on CPU from any of the
  three.
- **Hybrid retrieval and RRF.** SQLite FTS5 gives real BM25 to every language for free.
  RRF is fifteen lines of code.
- **The rebuild pattern** (hash-keyed embedding cache + atomic index swap). Pure design.
- **Chunking markdown.** qmd's scored-boundary algorithm is short enough to write anywhere.

### Genuinely language-constraining

| Constraint | Rust | Python | TypeScript |
| --- | --- | --- | --- |
| **`tantivy`** (best non-Postgres BM25 engine) | **Native** | `tantivy-py` | **No production binding** — use FTS5 instead (fine) |
| **Docling `HybridChunker`** (best PDF-structure chunker) | No | **Only** | No |
| **`sentence-transformers`** ecosystem, newest models day-one | Lags | **Canonical** | Lags |
| **PGlite** (only credible zero-install Postgres) | No | No | **Only** |
| **`postgresql_embedded`** (managed real-Postgres lifecycle) | **Only** | — | — |
| **`text-splitter`** | **Native** | Yes | No |
| **Chonkie** | `chonkie-rs` | **Canonical** | `chonkie-ts` (behind) |
| **Single-binary distribution** (no runtime, no venv) | **Yes** | Painful | Bun `--compile`, workable |
| **Model weights bundled/downloaded on first run** | Yes | Yes | Yes (qmd does exactly this) |

### The shape of the tension, stated plainly

- **Python's advantage is entirely upstream of search** — Docling for PDFs, the whole
  document-AI and OCR ecosystem, day-one access to new models. Given that §7 identifies
  PDF-structure chunking as the one place with a real library gap, and Centinel's corpus is
  PDF-heavy, this is not a small point.
- **Rust's advantage is entirely in distribution** — a single static binary that runs
  unattended with no runtime is worth a great deal for a tool aimed at civic volunteers, and
  Rust owns tantivy, LanceDB's core, and `postgresql_embedded` natively.
- **TypeScript's advantage is that qmd already proves the exact stack works there**, plus
  PGlite is the only real zero-install Postgres. The cost is losing tantivy and Docling.

A defensible resolution, if it helps: **the extraction/OCR pipeline and the search engine
do not have to be the same language.** The static file tree *is* the interface between
them. If PDF structure extraction is the only thing pulling toward Python, that can be a
separate tool writing markdown into the tree, and the search layer can be whatever else.

### Recommendations that hold regardless of language

1. **Do not make Postgres a hard dependency.** Define one storage interface with two
   implementations: **embedded (SQLite FTS5 + sqlite-vec, or LanceDB) as the default**, and
   **pgvector for the server deployment**. The operator's pgvector preference and the
   operator's qmd reference are both satisfiable; they just aren't the same backend. The
   hash-keyed embedding cache (§8) is what makes the two interchangeable — the vectors move,
   the index rebuilds.
2. **Hybrid by default, never pure vector.** BM25 costs nothing, and on a corpus of
   ordinance numbers, parcel IDs, and proper names it is not the fallback — it is often the
   better arm (§5).
3. **Cross-encoder reranking, opt-in per query.** The largest measured quality gain in this
   document (+18.6 nDCG@10 on BRIGHT) and the reason you can afford cheap first-stage
   retrieval. Follow qmd's three-command ergonomics: cheap default, expensive on request.
4. **Content-hash chunk identity.** Turns the versioned corpus from a 20× vector explosion
   into a ~1.6× one (§4), and makes incremental reindex trivial. Free, given the file tree
   is already hash-addressed.
5. **Model "change between versions" as an indexed object.** Similarity search cannot
   answer "what was removed" — VersionRAG measures naive RAG at **0 %** on that (§4). Diff
   records with their own embeddings are the fix, and "what changed" is the question this
   whole project exists to answer.
6. **Keep timestamps (transcripts) and page numbers (PDFs) on every chunk.** Citations are
   the product.
7. **Read [GovScape](https://arxiv.org/abs/2511.11010) before building.** 10 M government
   PDFs, 71 M pages, BGE + CLIP + olmOCR + FAISS/LanceDB + FTS5, ~$1,500 total. Closest
   existing system, open source, and it does *not* solve cross-crawl versioning — which is
   precisely where Centinel is differentiated and where §4 shows the field is genuinely
   under-served.

---

## Sources

- [tobi/qmd](https://github.com/tobi/qmd) · [README](https://raw.githubusercontent.com/tobi/qmd/main/README.md) · [DeepWiki architecture](https://deepwiki.com/tobi/qmd) · [ba0f3/qmd-go](https://github.com/ba0f3/qmd-go)
- [pgvector](https://github.com/pgvector/pgvector) · [pgvectorscale](https://github.com/timescale/pgvectorscale) · [VectorChord](https://github.com/tensorchord/VectorChord) · [ParadeDB / pg_search](https://github.com/paradedb/paradedb) · [Postgres FTS controls](https://www.postgresql.org/docs/current/textsearch-controls.html)
- [PGlite](https://pglite.dev/) · [`postgresql_embedded`](https://crates.io/crates/postgresql_embedded) · [theseus-rs/postgresql-binaries](https://github.com/theseus-rs/postgresql-binaries)
- [sqlite-vec](https://github.com/asg017/sqlite-vec) · [v0.1.0 release post w/ benchmarks](https://alexgarcia.xyz/blog/2024/sqlite-vec-stable-release/index.html)
- [LanceDB](https://github.com/lancedb/lancedb) · [Lance format](https://github.com/lance-format/lance) · [time-travel RAG tutorial](https://docs.lancedb.com/tutorials/agents/time-travel-rag) · [WikiSearch / 41 M docs FTS](https://www.lancedb.com/blog/feature-full-text-search)
- [Qdrant](https://github.com/qdrant/qdrant) · [Chroma](https://github.com/chroma-core/chroma) · [Weaviate](https://github.com/weaviate/weaviate) · [Milvus](https://github.com/milvus-io/milvus) · [tantivy](https://github.com/quickwit-oss/tantivy)
- [GovScape (arXiv 2511.11010)](https://arxiv.org/abs/2511.11010) · [VersionRAG (arXiv 2510.08109)](https://arxiv.org/html/2510.08109v1) · [LiveVectorLake (arXiv 2601.05270)](https://arxiv.org/html/2601.05270) · [Semantic Layers for Web Archives (arXiv 1810.10455)](https://arxiv.org/pdf/1810.10455) · [A time machine for text search (SIGIR '07)](https://dl.acm.org/doi/10.1145/1277741.1277831)
- [BRIGHT (arXiv 2407.12883)](https://arxiv.org/abs/2407.12883) · [Lighting the Way for BRIGHT (arXiv 2509.02558)](https://arxiv.org/html/2509.02558) · [LongEmbed (arXiv 2404.12096)](https://arxiv.org/abs/2404.12096) · [LoCo / M2-BERT (arXiv 2402.07440)](https://arxiv.org/html/2402.07440v2)
- [MTEB leaderboard](https://huggingface.co/spaces/mteb/leaderboard) · [RTEB announcement](https://huggingface.co/blog/rteb) · [MTEB v2](https://huggingface.co/blog/isaacchung/mteb-v2) · [embeddings-benchmark/mteb](https://github.com/embeddings-benchmark/mteb)
- [EmbeddingGemma-300m](https://huggingface.co/google/embeddinggemma-300m) · [Qwen3-Embedding-0.6B](https://huggingface.co/Qwen/Qwen3-Embedding-0.6B) · [Qwen3-Embedding-8B](https://huggingface.co/Qwen/Qwen3-Embedding-8B) · [Qwen3-Reranker-0.6B](https://huggingface.co/Qwen/Qwen3-Reranker-0.6B) · [bge-reranker-v2-m3](https://huggingface.co/BAAI/bge-reranker-v2-m3)
- [fastembed (Rust)](https://crates.io/crates/fastembed) · [fastembed (Python)](https://qdrant.github.io/fastembed/) · [fastembed-js](https://github.com/anush008/fastembed-js) · [model2vec](https://github.com/MinishLab/model2vec)
- [OpenAI pricing](https://developers.openai.com/api/docs/pricing) · [Voyage pricing](https://docs.voyageai.com/docs/pricing) · [Cohere pricing](https://cohere.com/pricing)
- [text-splitter](https://github.com/benbrandt/text-splitter) · [Chonkie](https://github.com/chonkie-inc/chonkie) · [chonkie-rs](https://crates.io/crates/chonkie) · [chonkie-ts](https://github.com/chonkie-inc/chonkie-ts) · [Docling chunking](https://docling-project.github.io/docling/concepts/chunking/)
- [Action-Item-Driven Summarization of Long Meeting Transcripts (arXiv 2312.17581)](https://arxiv.org/pdf/2312.17581) · [Hierarchical Text Segmentation Chunking for RAG (arXiv 2507.09935)](https://arxiv.org/pdf/2507.09935) · [Diarization-Aware Multi-Speaker ASR (arXiv 2506.05796)](https://arxiv.org/pdf/2506.05796)
