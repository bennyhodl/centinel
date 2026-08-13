# Retrieval

How a question becomes a cited passage.

Everything here runs on the machine in front of you. No embedding API, no reranking API,
no text leaving the process (SPEC §2.1). Two model files and two files on disk.

```
                        centinel index
  derived text  ──────────────────────────►  centinel.db      chunk + placement + FTS5
        │
        │       centinel embed
        └──────────────────────────────────►  vectors.lance/  chunk_hash → vector

  query
    ├─ BM25   (SQLite FTS5)          → top 100    instant, no model
    └─ vector (Qwen3-Embedding-4B)   → top 100    one embed call
          └─ RRF fuse (k=60)         → top 40
                └─ Qwen3-Reranker    → top n      always on
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
<root>/vectors.lance/     LanceDB: chunk vectors    — the vector arm
```

Both are **derived**. Only `blobs/` and `log/` are truth; delete either store and it
rebuilds from them.

Derived does not mean cheap. `centinel.db` rebuilds in minutes. `vectors.lance/` is
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
centinel embed --dry-run          # what would be embedded, without loading a model
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

### The model

**`Qwen3-Embedding-4B`**, Q8_0 GGUF, **2,560 dimensions**, 32K context, Apache-2.0 and
published by Qwen themselves. Run in-process through `llama-cpp-2` — no server, no
sidecar, no second language runtime.

Licence decided the family: Centinel auto-downloads weights and forks redistribute them,
which rules out EmbeddingGemma (Gemma licence) and Jina's reranker (CC-BY-NC).

Size is decided by **where the cost lands**. The embedder is paid once per corpus in
hours; the reranker is paid per query in milliseconds. On MTEB English Retrieval,
0.6B scores 61.83, 4B scores 68.46 and 8B scores 69.44 — nearly the whole gain is
0.6B→4B, and 8B buys +1.0 point for roughly double the embedding time. So the budget goes
into the embedder once and into the reranker freely (SPEC §6.2).

Q8_0 over Q4_K_M for the same reason: quantization here is amortised over hours of work
rather than paid per query.

### The recipe, and why it is written down

Three things that a generic embedding wrapper would get wrong, and none of them errors:

1. **Last-token pooling**, not mean pooling.
2. **An instruction prefix on queries only.** A query is wrapped as
   `Instruct: {task}\nQuery:{q}`; a document is embedded bare. The asymmetry is the
   model's — it was trained to treat the relationship as directional.
3. **L2 normalization**, so cosine similarity is a dot product.

Each produces *plausible* vectors when wrong: slightly worse retrieval, no error
anywhere. `embed.rs` therefore carries a test that asserts on semantics rather than on
shapes.

### Batching is not optional

A batch is **one forward pass over many chunks**: one context, one `decode`, every chunk
as its own `seq_id`. Two costs collapse into it. First, a `llama.cpp` context and its KV
cache are built per **call**, not per text:

| | chunks/sec (M1 Max, Metal) |
|---|---|
| one chunk per call | 6.1 |
| batches of 32 | **18.5** (0.6B) / **3.8** (4B) |

Second, one ~300-token chunk leaves a GPU almost entirely idle. The table measures only
the first — it predates the packed decode and has not been re-run against it.

So the batch is the unit of work, not the chunk. A batch that fails as a unit — an
over-long chunk, or a group the machine cannot hold — is retried individually, so one bad
chunk cannot cost the other 31. How wide is a property of the machine: `[defaults]
embed_batch`, `--batch N`, or `auto`.

An over-long text is **refused, not truncated**. A silently shortened chunk would be
stored under a `chunk_hash` covering text that was never embedded, which makes the record
lie about what it holds.

The whole run goes into a single `spawn_blocking`. Inference would otherwise stall the
async runtime, which matters here because an HTTP caller's connection has to survive a
multi-hour run.

### Resumability is a consequence, not a feature

No checkpoint file. The work list is

```
index chunk hashes  −  stored chunk hashes
```

Kill it at chunk 40,000 and re-run; it starts at 40,001. Lance commits a version per
append, so what landed before the kill is there.

`--dry-run` creates no table. A plan must leave nothing behind.

### What it costs

On a 400,000-chunk corpus at the measured 3.8 chunks/sec: **about a day**, once. A
monthly recrawl re-embeds only what changed.

---

## 4. The table

```
vectors.lance/    one table, two columns
                    chunk_hash  Utf8
                    vector      FixedSizeList<Float32, dims>
```

No text, no placements, no source. `centinel.db` holds those, and a second copy goes out
of date the first time the corpus changes. `chunk_hash` is the join both stores already
use.

**The model is a property of the table.** Its id lives in the schema metadata, and a
query vector from any other model is refused at open, naming the fix. Vectors from two
models are in different spaces and still return a confident ranked list — there is no
symptom but a worse ordering. Width is guarded by the schema itself: the column is a
fixed-size list of exactly `dims` floats, so a wrong width cannot be written at all.

This also means `search` is never *told* which embedder to use. It asks the table. A
reader configured differently would otherwise have its query refused and quietly fall
back to one arm.

**One table, not one per model.** SPEC §6.2 already makes changing the embedder a full
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
useless. That case is asserted as a test, not described.

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

**`Qwen3-Reranker-0.6B`**, Q8_0 GGUF, 32K context, Apache-2.0. The weights are a
community conversion by `ggml-org` — the llama.cpp organisation — because Qwen publish
GGUF for the embedder only. Digests pin exactly what is fetched.

One command, one answer, and **no fast path that silently returns worse results**. The
measured gap is why: BM25 goes from **14.8 to 33.4** nDCG@10 when reranked, and reranked
BM25 beats an expensively-trained reasoning-tuned dense retriever used alone at 29.1. A
default that returned the 14.8 would be a footgun.

The architectural consequence is the reason the first stage can be cheap: **a cheap stage
that over-fetches plus a good reranker beats an expensive retriever alone.** The first
stage only has to get the right passage into the top 40. It does not have to rank it.

**It is not an embedding model, and that is the whole difficulty.** `Qwen3-Reranker` is a
causal language model. It emits no vector. It is asked a yes/no question and the answer is
read from the logits at the final position:

```
score = softmax([logit(no), logit(yes)])[1]     →  P(yes), in [0, 1]
```

So it runs pooling `None` and reads logits, where the embedder runs pooling `Last` and
reads embeddings. They share a runtime and nothing else.

Three things must be exactly right, and none of them errors when wrong:

1. **The chat template, verbatim** — the system line included. A hand-rolled prompt gets a
   fluent answer to a different question.
2. **`yes` and `no` as single tokens.** More than one piece and the logit being read
   belongs to a fragment. Checked at load, because a model that fails this cannot be
   scored at all.
3. **Softmax over exactly those two logits**, not over the vocabulary. The absolute logits
   drift with document length; their difference does not.

The softmax is written as a difference, `1 / (1 + exp(no - yes))`, rather than two `exp`
calls over a shared denominator: raw logits can be large enough to overflow to infinity,
which yields `NaN` and an order that depends on the sort's tie-breaking.

Unlike `embed`, an over-long document here is **truncated, not refused**. A passage is one
candidate among many and a shortened judgement is still a judgement, where refusing would
drop a result the first stage chose. Nothing here is stored, so nothing can lie about what
it covers.

### `--source`

The BM25 arm filters in SQL. The vector arm cannot: Lance carries no source column, and a
chunk has many placements across sources. So it **over-fetches 5×, then post-filters** in
one query for the whole candidate set. It can still under-fill on a corpus one source
dominates — a known limit of the post-filter, not a bug in it.

---

## 6. What a result tells you about itself

A rank is a position **inside** a set. It says nothing about the size of that set.

RRF weights by rank alone, so the vector arm's rank 1 counts exactly the same whether it
was drawn from 397,830 vectors or from 2,309. **A partly embedded corpus therefore does
not degrade gently** — it promotes confident results from a tiny pool and looks identical
to a complete one.

This is the same error shape as `pages_needing_ocr`: a chunk's absence from an arm is a
fact about what has been *processed*, never about whether it answers the question.

So every report says what actually happened:

| field | what it carries |
|---|---|
| `method` | which stages ran: `bm25`, `bm25→rerank`, `bm25+vector→rrf`, `bm25+vector→rrf→rerank` |
| `total_chunks_indexed` | the corpus |
| `vectors_indexed` | how much of it the vector arm could see |
| `no_vectors` | why the vector arm did not run |
| `no_rerank` | why the ordering was not reranked |

`method` is assembled from what ran, never hard-coded — it is the one field a reader
trusts to know what they are looking at, and a stale literal there is worse than no field.
The terminal prints the coverage share whenever it is not 100%.

```
stormwater drainage fee    2 results · bm25→rerank · 397,830 chunks indexed
! keyword search only — no vectors at ~/.centinel/vectors.lance — run `centinel embed` first
```

**Always on is not the same as always available.** §6.3 forbids a *flag* that silently
returns worse results. It does not promise that a machine with no reranker weights refuses
to search. Missing weights degrade the answer and say so; they never turn a query into an
error a reader cannot act on. The same holds for an unbuilt vector table: a corpus is
keyword-searchable long before it is embedded.

Every result leads its provenance line with a **handle** — the short blob hash — because
anything Centinel prints, Centinel takes back. `centinel read <hash>` and
`centinel open <hash>` accept it by prefix.

---

## 7. What it uses

| | |
|---|---|
| **embedder** | `Qwen3-Embedding-4B` Q8_0 GGUF · 2,560-dim · Apache-2.0 · first-party |
| **reranker** | `Qwen3-Reranker-0.6B` Q8_0 GGUF · Apache-2.0 · `ggml-org` conversion |
| **runtime** | `llama-cpp-2` in-process · Metal on macOS, CUDA/Vulkan/ROCm opt-in |
| **vectors** | `lancedb` 0.33, no default features |
| **keywords** | SQLite FTS5, `bundled` |
| **fusion** | ours — a few dozen lines |

GGUF and not ONNX because the `onnx-community` exports are decoder graphs carrying a KV
cache, and CoreML refuses tensors with zero elements — which is exactly what an empty
cache is. That makes ONNX permanently CPU-only on Apple Silicon. Measured on the same
model both ways: ONNX on CPU gives 5.5 chunks/sec, `llama.cpp` on Metal gives 18.5
(SPEC §6.2.1).

---

## 8. Cost, in one place

| | |
|---|---|
| embed a 400k-chunk corpus | ~1 day, once |
| re-embed after a monthly recrawl | ~5% of that |
| rebuild `centinel.db` | minutes |
| disk, vectors at 2,560-dim | 3.79 GiB per 400k chunks |
| a query, warm process | ~1 second (the reranker) |
| a query, cold CLI | **11 s measured** with the reranker alone; the embedder adds its own load on top |

The last row is worth knowing, and the measurement is honest about what it covers: 11.35 s
on the Tampa corpus with no vector table, so only the 0.6B reranker was loaded. A query
that also builds the 4B embedder pays more. `centinel serve` and `centinel mcp` load both
once; a short CLI invocation pays on every query.

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
