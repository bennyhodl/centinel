# Retrieval

How a question becomes a cited passage.

Centinel runs no model itself. Embedding and reranking are each a request to a server
the operator names in `[inference]` — by default llama-server on `127.0.0.1:8080`, so
with the default nothing leaves the machine (SPEC §2.1 as amended). Two servers and two
files on disk.

A `base_url` elsewhere — OpenRouter, a GPU box across the room — sends chunk text, and
at search time the query, there instead. The config file is the one place that says so,
and nothing falls back to a server the operator did not name. See §3 below. Everything
else on this page — blobs, logs, provenance, the index — stays local either way.

```
                        centinel index
  derived text  ──────────────────────────►  centinel.db      chunk + placement + FTS5
        │
        │       centinel embed
        └──────────────────────────────────►  chunks.lance/   one row per chunk placed in a document

  query
    ├─ BM25   (SQLite FTS5)          → top 100    instant, no model
    └─ vector (embed server)         → top 100    one request
          └─ RRF fuse (k=60)         → top 40
                └─ rerank server     → top n      always on
```

---

## 1. The unit is a chunk, and its identity is its text

A **chunk** is a passage of derived text. Its id is `chunk_hash` — the SHA-256 of the text
itself, not of the document it came from, not of the address it was found at.

Everything downstream falls out of that one choice.

| | |
|---|---|
| target size | **1,200 characters** (~300 tokens) |
| overlap | **150 characters** |
| minimum | 80 characters |

The same passage on fifty pages is **one** row in `chunk` and fifty rows in `placement`.
A council's standard notice paragraph is embedded once, not fifty times. And a monthly
recrawl of a site that is ~95% unchanged produces ~95% identical hashes, so `embed` only
sees what genuinely changed (SPEC §6.1).

**Chunk geometry is load-bearing far outside chunking.** `chunk_hash` hashes the chunk's
*text*, and the geometry decides the text. Change the target or the overlap and you get a
wholly different set of hashes — the old chunks stay in the index and every vector is
orphaned. The index records the geometry its hashes were built with and refuses a change
that is not a rebuild.

A **placement** is where a chunk sits: source, address, derived blob, character span,
heading trail, observation time, and the tool that derived it. That is what makes a
result citable, and it is why the address is part of a placement's identity rather than
the derived blob alone — two pages can extract to byte-identical text.

---

## 2. Two stores, deliberately independent

```
<root>/centinel.db        SQLite: metadata + FTS5   — the BM25 arm
<root>/chunks.lance/      LanceDB: chunk vectors    — the vector arm
```

Both are **derived**. The truth is `blobs/` and `log/`, plus `workspace/workspace.db` for
what classifiers and people decided about the documents; delete either store and it
rebuilds from them.

Derived does not mean cheap. `centinel.db` rebuilds in minutes. `chunks.lance/` is
inference over the whole corpus — on 400,000 chunks, about a day. Nothing evidentiary is
lost either way, and one of them is a very expensive thing to delete by accident. **Its
backup is `cp -R`.**

The two arms stay independent on purpose: either rebuilds without touching the other.
That is why the BM25 arm remains on SQLite FTS5 rather than moving to LanceDB's own
Tantivy index, even though LanceDB ships one — and why LanceDB's built-in RRF reranker
goes unused. It can only fuse arms Lance owns (SPEC §6.4).

### There is no embedding cache

SPEC §5.2 originally specified one: a durable, portable, append-only file of vectors
beside the static files, on the argument that *"swapping vector backends is a re-import,
not a re-embed."*

**That was reversed after measurement.** A `.lance` dataset is an ordinary directory — a
manifest, a transaction log, data files. `cp -R` copies it, the copy opens and queries,
and a plain scan reads every vector back out. Extracting vectors from Lance *is* the
re-import. Publishing is a directory copy, and so is backup. Lance's transaction log
handles an interrupted run better than truncating a torn record did.

What the cache cost was a second write path and a pipeline stage with its own skip
predicate — and a wrong skip predicate is the defect that has cost this project the most.
So `embed` writes vectors where `search` reads them.

---

## 3. `centinel embed` — the expensive stage

```bash
centinel embed --dry-run          # what would be embedded, without asking the server
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

### The server

```toml
[inference]
base_url = "http://127.0.0.1:8080/v1"   # the default; llama-server's default port
[inference.embed]
model = "qwen3-embedding-4b"
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
batch = 64          # chunks per request
concurrency = 4     # requests in flight
```

```bash
llama-server -hf Qwen/Qwen3-Embedding-4B-GGUF:Q8_0 --embedding --pooling last
```

The request is OpenAI's `POST {base_url}/embeddings`, which llama-server (`--embedding`),
Ollama, vLLM, LM Studio and OpenRouter all answer. What the server does to produce a
vector — quantization, pooling, how the work is packed onto a GPU — is its tuning, set
once for its machine. What comes back is held to one contract whichever server sent it:
one vector per input, ordered by each row's `index` rather than by arrival, every one the
same width and finite. A server that answers at another width is serving another model,
and the run ends there rather than skipping the corpus a batch at a time.

### The model

The default is **`Qwen3-Embedding-4B`**, Q8_0 GGUF, **2,560 dimensions**, 32K context,
Apache-2.0 and published by Qwen themselves.

Licence decided the family when Centinel downloaded weights itself and forks
redistributed them, which ruled out EmbeddingGemma (Gemma licence) and Jina's reranker
(CC-BY-NC). The default has not moved since; a server can now run either, and that is
the operator's licence to read.

Size is decided by **where the cost lands**. The embedder is paid once per corpus in
hours; the reranker is paid per query in milliseconds. On MTEB English Retrieval,
0.6B scores 61.83, 4B scores 68.46 and 8B scores 69.44 — nearly the whole gain is
0.6B→4B, and 8B buys +1.0 point for roughly double the embedding time. So the budget goes
into the embedder once and into the reranker freely (SPEC §6.2).

Q8_0 over Q4_K_M for the same reason: quantization here is amortised over hours of work
rather than paid per query. It is chosen on the server's command line now, and the
reason still holds.

### The recipe, and why it is written down

Three things a generic embedding setup gets wrong, and none of them errors:

1. **Last-token pooling**, not mean pooling. The server's to apply — llama-server's
   `--pooling last`.
2. **An instruction prefix on queries only.** A query is wrapped as
   `Instruct: {task}\nQuery:{q}`; a document is embedded bare. The asymmetry is the
   model's — it was trained to treat the relationship as directional. The server cannot
   know a model wants this, so `[inference.embed] query_prefix` states it, and a written
   `[inference.embed]` table must state it, even as `""` for a symmetric model.
3. **L2 normalization**, so cosine similarity is a dot product. Done in `embed.rs` rather
   than trusted: normalizing a unit vector is a no-op, and `search` relies on the dot
   product.

Each produces *plausible* vectors when wrong: slightly worse retrieval, no error
anywhere. `embed.rs` therefore carries tests that assert the prefix lands on queries only
and that every vector leaves at unit length, whatever the server sent.

### Batching, and requests in flight

A request carries `batch` chunks (64 by default, `--batch N` for one run), and
`concurrency` requests are in flight at once: a reader thread keeps chunk text coming off
SQLite, the requests hide the round trip, and a writer task appends what comes back — so
the server waits on neither SQLite nor Lance. How the server packs a request onto its
GPU is its own tuning, not this binary's flags.

When models ran in-process the batch was one `llama.cpp` forward pass, and batching was
measured as the whole game:

| | chunks/sec (M1 Max, Metal) |
|---|---|
| one chunk per call | 6.1 |
| batches of 32 | **18.5** (0.6B) / **3.8** (4B) |

The same holds behind a server, which is why requests are wide rather than one chunk
each.

A batch that fails is **bisected**, so healthy chunks still go in groups: a window of 64
with one chunk the server will not take costs a dozen requests, not 64, and the one chunk
is reported as skipped. A failure that will refuse every half the same way — nothing
listening, a key refused, a model the server does not have — is not bisected; the run
ends on it.

Nothing is truncated here. A silently shortened chunk would be stored under a
`chunk_hash` covering text that was never embedded, which makes the record lie about
what it holds.

### A server elsewhere

```toml
[inference.embed]
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"   # the variable's name, never the key
model = "qwen/qwen3-embedding-8b"
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
```

Each capability may name its own `base_url` and `api_key_env`. Chunk text, and at search
time the query, go to that server; nothing else leaves — blobs, logs, provenance and the
index stay put. The work list, the resume subtraction and the skip accounting are the
same whichever server answers. The reason to reach for one: `qwen/qwen3-embedding-8b` —
too big for most laptops — costs about $0.01 per million tokens on OpenRouter, so a
400,000-chunk corpus embeds for a few dollars in well under a day, GPU or no GPU.

A different server is a different **vector space**, even for the same model name (§4):
quantization and pooling differ, and the difference shows up only as worse rankings.
Moving a table there is either a full re-embed or `centinel embed --adopt`, which
measures that the new server reproduces what is stored.

The run never blocks the async runtime: the requests are awaited, and SQLite reads sit
on their own thread. That matters because an HTTP caller's connection has to survive a
multi-hour run.

### Resumability is a consequence, not a feature

No checkpoint file. The work list is

```
index chunk hashes  −  stored chunk hashes
```

Kill it at chunk 40,000 and re-run; it starts at 40,001. Lance commits a version per
append, so what landed before the kill is there.

The subtraction runs the other way too. Before a run embeds anything it deletes the rows
whose hash the index no longer has — what a rebuilt index or a `--rebuild --source`
leaves behind — so one stage owns the table in both directions and a stored vector has
one meaning: a row whose hash is in the index. The report says how many were `stale` and
how many it `pruned`. An empty index prunes nothing; it is an index that has not been
built, not a verdict on the vectors. After the run the table is compacted and versions
older than ten minutes are dropped, which is what keeps a query from reading one fragment
per batch ever appended (one store reached 20,000 fragments and 16 GiB of manifests).

`--dry-run` creates no table and prunes nothing. A plan must leave nothing behind.

### What it costs

On a 400,000-chunk corpus at the 3.8 chunks/sec measured in-process on an M1 Max: **about
a day**, once. Behind a server the speed is that machine's, and it has not been
re-measured. A monthly recrawl re-embeds only what changed.

---

## 4. The table

```
chunks.lance/     one table, one row per chunk placed in a document
                    key          Utf8      chunk_hash ␟ source ␟ resource ␟ derived_sha
                    chunk_hash   Utf8
                    source, resource, derived_sha   Utf8
                    tags         List<Utf8>          what the workspace's policy says of the document
                    excluded     Boolean
                    vector       FixedSizeList<Float32, dims>
```

No text. `centinel.db` holds that, and a second copy goes out of date the first time the
corpus changes. What the row does carry is the document and what the workspace decided
about it, because that is what a search is narrowed to, and a filter that lives on the row
is applied inside the vector query. The same passage on fifty pages is fifty rows with one
vector; `embed` copies it rather than asking the model again. When a document's tags or
exclusion change, the workspace queues it and a sync rewrites those two columns in place —
Lance patches them inside the existing fragments and never touches a vector. A store from
before this layout holds `vectors.lance/`, one row per chunk. This release no longer reads
it — a copy matched on a model name alone would skip the measurement `--adopt` makes — and
refuses with the way forward: upgrade it with the release before this one, whose `centinel
embed --limit 0` fills `chunks.lance/` with nothing sent to a model, or delete it and embed
again.

**The space is a property of the table.** The schema metadata records the model and the
server that ran it — `centinel.model_id` and `centinel.endpoint`, the second absent on a
table written before `[inference]` existed — and rows or a query vector from any other
space are refused, naming the fix. The server is part of the space because a model is
only ever a model *as served*: the same weights at two quantizations, or behind two
servers' pooling, are close and not identical. Vectors from two spaces still return a
confident ranked list — there is no symptom but a worse ordering. Width is guarded by the
schema itself: the column is a fixed-size list of exactly `dims` floats, so a wrong width
cannot be written at all. A new table takes its width from one probe request to the
server.

`search` reads the table's space and checks the configured embed server against it. When
they differ the vector arm does not run, and `no_vectors` says so and points at
`centinel embed --adopt`, rather than ranking the corpus in a space it was never written
in.

**Moving a table to another server.** `centinel embed --adopt` samples up to 32 stored
vectors whose chunks the index still has, embeds those chunks on the configured server,
and adopts the server — rewriting the metadata in one manifest commit, no row touched —
only if every pair agrees at cosine ≥ 0.99. The same weights behind a new server land at
0.999 and above; a different model, or the same one at a coarser quantization, lands
well under on most chunks. Otherwise it refuses with the minimum and mean it measured and
what to run instead: the old server again; for an `openrouter/…` id, the OpenRouter
config; for a table embedded in-process as `qwen3-embedding-4b` or `0.6b`, the
`llama-server` command that runs the same Q8_0 weights with last-token pooling; or delete
`chunks.lance/` and re-embed. `--adopt --dry-run`
measures without writing.

**One table, not one per space.** SPEC §6.2 already makes changing the embedder a full
re-embed rather than a config edit, so a second model is a rebuild.

**ANN indexing is not built yet.** With no index, Lance scans flat — exact, and the right
answer while the table is small. An `IVF_PQ` index starts to earn its cost somewhere above
roughly 100,000 rows, which is a threshold to measure rather than a number to trust. A
full corpus is 397,830 × 2,560 × 4 bytes = **3.79 GiB**.

Dependency: `lancedb = { version = "0.33.0", default-features = false }`. The default
features are the S3, GCS, Azure and OSS object stores — every one a network path out of a
machine that §2.1 says nothing leaves.

---

## 5. `centinel search` — two arms, fused, reranked

```bash
centinel search "stormwater drainage fee"
centinel search "budget" --source tampa -n 20
```

### Why both arms

Neither is a warm-up.

**BM25 catches exact tokens.** Names, motions, ordinance numbers, dollar figures — what
people actually search meeting records for. On the BRIGHT benchmark BM25 scores 13.7
against BGE-large's 13.8. Vector-only search fails hardest on precisely these.

**The vector arm closes the vocabulary gap.** Measured on the real corpus: `"drinking
water sampling results"` returns **nothing** from FTS5, because the water report says
`PWSName`, `Analyte` and `UCMR 5`, and the only chunk containing "drinking" is a tax table
about *Drinking Places (Alcoholic Beverages)*. BM25 is behaving correctly and is still
useless. That case is asserted as a test, not described — against real servers, so it is
opt-in (`CENTINEL_TEST_INFERENCE=1`).

### RRF

```
score(chunk) = Σ  1 / (60 + rank_in_arm)
```

Top 100 from each arm, fused on `chunk_hash`, top 40 kept.

Rank-based on purpose. The two arms produce scores on incomparable scales — FTS5's
negated BM25 against a cosine similarity — and normalising them into one number is a
hidden weighting. Ranks are what the arms genuinely agree on. `k = 60` keeps the gap
between rank 1 and rank 2 small, so **agreement between the arms matters more than either
arm's confidence**, which is the whole reason to fuse rather than pick.

Ties break on `chunk_hash`, so the same query twice returns the same order. A `HashMap`
iterates arbitrarily, and two equal-scoring chunks swapping places between runs reads as
the corpus having changed.

### Reranking, always on

```toml
[inference.rerank]
model = "qwen3-reranker-0.6b"
```

The default is **`Qwen3-Reranker-0.6B`**, 32K context, Apache-2.0. Qwen publish GGUF for
the embedder only, so the GGUF a llama-server runs for it is a community conversion by
`ggml-org`, the llama.cpp organisation.

One command, one answer, and **no fast path that silently returns worse results**. The
measured gap is why: BM25 goes from **14.8 to 33.4** nDCG@10 when reranked, and reranked
BM25 beats an expensively-trained reasoning-tuned dense retriever used alone at 29.1. A
default that returned the 14.8 would be a footgun.

The architectural consequence is the reason the first stage can be cheap: **a cheap stage
that over-fetches plus a good reranker beats an expensive retriever alone.** The first
stage only has to get the right passage into the top 40. It does not have to rank it.

**The request has no OpenAI spec.** OpenAI publishes no rerank endpoint, so this speaks
the shape Cohere and Jina published and everyone else copied — llama-server under
`--reranking`, vLLM, TEI, Infinity, LocalAI:

```
POST {base_url}/rerank   {"model", "query", "documents": [...], "top_n"}
→                        {"results": [{"index", "relevance_score"}, ...]}
```

Scores are read by each row's `index`, because a server answers best first. Every
document must come back scored, once, with a finite score, or the response is refused
rather than paired up arbitrarily. The scores are in whatever range the server uses; only
their order is relied on.

**It is not an embedding model, and that is the server's difficulty now.**
`Qwen3-Reranker` is a causal language model. It emits no vector. It is asked a yes/no
question and the answer is read from the logits at the final position:

```
score = softmax([logit(no), logit(yes)])[1]     →  P(yes), in [0, 1]
```

When it ran in-process, three things had to be exactly right, and none of them errored
when wrong: the chat template verbatim, system line included; `yes` and `no` as single
tokens, checked at load; and a softmax over exactly those two logits, written as
`1 / (1 + exp(no - yes))` so a large raw logit could not overflow to `NaN`. They are what a
rerank server applies for this model, and they are the first thing to check when a
server's ordering looks wrong.

Nothing a reranker sees is stored, so however a server handles an over-long passage —
truncating it or refusing it — nothing can lie about what it covers.

### `--source`, `--tag`, `--not-tag`

```
centinel search "stormwater" --source hillsborough
centinel search "stormwater" --tag record_type:minutes --tag water_environment
centinel search "permits" --not-tag record_type:data_table
```

One filter, applied the same way by both arms, each inside its own query. The BM25 arm
applies it in SQL against the workspace database the index attaches. The vector arm
applies it in Lance: every row carries its document's source, tags and exclusion, so the
query asks for `excluded = false`, the source, `array_has_all(tags, …)` and
`NOT array_has_any(tags, …)` before distance is computed, and comes back with its full
depth in distinct chunks however narrow the filter. The placements a hit cites are filtered
by the same clauses on the way out, so a passage shared by two documents never names one
the filter left out, and a hit the vector arm found cites the document it was admitted
through first.

What a filter costs: with an IVF index, Lance reads the probed partitions, masks the rows
the filter rejects, and scores the rest. The masked rows cost no arithmetic, but their
partitions are still read, so a search narrowed to one source of thirty reads about as many
bytes as an unfiltered one. `examples/vector_bench.rs` measures the filtered case beside the
plain one.

A tag is what the classifier workspace's policy put on a document: a yes-or-no question's
id, or `choice:option`. Repeated `--tag` is AND. A tag no saved question defines is an
error, not an empty result. Every result carries the `tags` of the document it cites, so a
reader sees why a filtered hit qualified and what else it is. Tags are live: a threshold
moved in the web page re-decides the stored answers at once, and the next `centinel search`
sees it without a re-score. See [CLASSIFIERS.md](CLASSIFIERS.md).

---

## 6. What a result tells you about itself

A rank is a position **inside** a set. It says nothing about the size of that set.

RRF weights by rank alone, so the vector arm's rank 1 counts exactly the same whether it
was drawn from 397,830 vectors or from 2,309. **A partly embedded corpus therefore does
not degrade gently** — it promotes confident results from a tiny pool and looks identical
to a complete one.

This is the same error shape as `pages_needing_ocr`: a chunk's absence from an arm is a
fact about what has been *processed*, never about whether it answers the question.

So every report says what actually ran:

| field | what it carries |
|---|---|
| `method` | which stages ran: `bm25`, `bm25→rerank`, `bm25+vector→rrf`, `bm25+vector→rrf→rerank` |
| `no_vectors` | why the vector arm did not run |
| `no_rerank` | why the ordering was not reranked |

`method` is assembled from what ran, never hard-coded — it is the one field a reader
trusts to know what they are looking at, and a stale literal there is worse than no field.

```
stormwater drainage fee    2 results · bm25→rerank
! keyword search only — no vectors at ~/.centinel/chunks.lance — run `centinel embed` first
```

What the report does not say is how much of the corpus the vector arm could see. It used
to carry a chunk count beside the vector count and print the share; the count was a
`COUNT(*)` over every chunk, 99 s cold at 21.7 million, paid by every search, and the
share ignored `--source` and `--tag`, counted stale vectors, and read as zero whenever
the vector arm failed. Coverage is a set difference — indexed chunks against stored
vectors — and `centinel embed --dry-run` computes exactly that: `indexed`,
`already_embedded`, `remaining`, `stale`.

**Always on is not the same as always available.** §6.3 forbids a *flag* that silently
returns worse results. It does not promise that a machine whose rerank server is down
refuses to search. A server that is not answering degrades the answer and says so, in
`no_rerank` or `no_vectors`; it never turns a query into an error a reader cannot act on.
Whether the rerank server is there is asked before the hits are looked at, so a search
that found nothing still says the reranker is missing. The same holds for an unbuilt
vector table: a corpus is keyword-searchable long before it is embedded.

Every result leads its provenance line with a **handle** — the short blob hash — because
anything Centinel prints, Centinel takes back. `centinel read <hash>` and
`centinel open <hash>` accept it by prefix.

---

## 7. What it uses

| | |
|---|---|
| **embedder** | default `Qwen3-Embedding-4B` · 2,560-dim · Apache-2.0 · first-party |
| **reranker** | default `Qwen3-Reranker-0.6B` · Apache-2.0 · `ggml-org` GGUF conversion |
| **runtime** | a server the operator runs — llama-server by default — over HTTP |
| **vectors** | `lancedb` 0.33, no default features |
| **keywords** | SQLite FTS5, `bundled` |
| **fusion** | ours — a few dozen lines |

No model runtime is linked into the binary. The tuning that makes a model fast on one
machine — GPU layers, physical batch, flash attention, threads — belongs to the server,
set once for that machine, so a release is one plain CPU binary per platform.

When models ran in-process, the runtime was chosen by measurement: GGUF through
`llama.cpp` and not ONNX, because the `onnx-community` exports are decoder graphs
carrying a KV cache, and CoreML refuses tensors with zero elements — which is exactly what
an empty cache is. That made ONNX permanently CPU-only on Apple Silicon. Measured on the
same model both ways: ONNX on CPU gave 5.5 chunks/sec, `llama.cpp` on Metal gave 18.5
(SPEC §6.2.1).

---

## 8. Cost, in one place

| | |
|---|---|
| embed a 400k-chunk corpus | ~1 day, once |
| re-embed after a monthly recrawl | ~5% of that |
| rebuild `centinel.db` | minutes |
| disk, vectors at 2,560-dim | 3.79 GiB per 400k chunks |
| a query | one embed request and one rerank request, at the servers' speed |

When models ran in-process a warm query took ~1 second, most of it the reranker, and a
cold CLI invocation **11.35 s measured** — on the Tampa corpus with no vector table, so
only the 0.6B reranker was loaded, and the 4B embedder added its own load on top. A
server loads its model once, so a short CLI invocation no longer pays that; what a query
costs now is the server's time on the rerank, which has not been re-measured.

---

## 9. Not built yet

- **ANN indexing.** Flat scan is exact and fine below ~100k rows.
- **A source column on the vector table.** A `List<Utf8>` would push `--source` down into
  Lance, at the cost of updating a row whenever a new placement appears for an existing
  chunk.
- **MRL truncation.** Qwen3-Embedding is Matryoshka, so a narrower index is a prefix slice
  of a stored vector rather than a re-embed — a reversible decision, deferred until
  something measures a need for it.
- **Transcript-aware chunking** (SPEC §6.5) — agenda-aligned spans and per-chunk
  timestamps, which is what turns a hit into a `watch?v=X&t=4271s` citation.

---

**See also:** [`ARCHITECTURE.md`](ARCHITECTURE.md) for the store and the op registry ·
[`SPEC.md`](SPEC.md) §5–§6 for the locked decisions and their accepted costs ·
[`../CONTEXT.md`](../CONTEXT.md) for the vocabulary these terms come from ·
[`research/semantic-search.md`](research/semantic-search.md) for the evidence underneath.
