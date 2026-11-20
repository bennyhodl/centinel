# Search

How a question becomes a cited passage.

```
query
  ├─ BM25   (SQLite FTS5)          → top 100    instant, no model
  └─ vector (Qwen3-Embedding-4B)   → top 100    one embed call
        └─ RRF fuse (k=60)         → top 40
              └─ Qwen3-Reranker-0.6B → top n    always on
```

Everything here runs on the machine in front of you. Two model files and two files on
disk.

## Why both arms

Neither is a warm-up.

**BM25 catches exact tokens.** Names, motions, ordinance numbers, dollar figures — what
people actually search meeting records for. On the BRIGHT benchmark BM25 scores 13.7
against BGE-large's 13.8. Vector-only search fails hardest on precisely these.

**The vector arm closes the vocabulary gap.** Measured on the real corpus: `"drinking
water sampling results"` returns **nothing** from FTS5, because the water report says
`PWSName`, `Analyte` and `UCMR 5`, and the only chunk containing "drinking" is a tax table
about *Drinking Places (Alcoholic Beverages)*. BM25 is behaving correctly and is still
useless. That case is asserted as a test, not described in a comment.

## RRF

```
score(chunk) = Σ  1 / (60 + rank_in_arm)
```

Top 100 from each arm, fused on `chunk_hash`, top 40 kept.

Rank-based on purpose. The two arms produce scores on incomparable scales — FTS5's negated
BM25 against a cosine similarity — and normalising them into one number is a hidden
weighting. Ranks are what the arms genuinely agree on.

`k = 60` keeps the gap between rank 1 and rank 2 small, so **agreement between the arms
matters more than either arm's confidence**, which is the whole reason to fuse rather than
pick.

Ties break on `chunk_hash`, so the same query twice returns the same order. A `HashMap`
iterates arbitrarily, and two equal-scoring chunks swapping places between runs reads as
the corpus having changed.

## Reranking, always on

**`Qwen3-Reranker-0.6B`**, Q8_0 GGUF, 32K context, Apache-2.0, run by the server
`[inference.rerank]` names — `llama-server --reranking` is the recommended one. The
recommended weights are a community conversion by `ggml-org` — the llama.cpp organisation —
because Qwen publish GGUF for the embedder only.

One command, one answer, and **no fast path that silently returns worse results**. The
measured gap is why: BM25 goes from **14.8 to 33.4** nDCG@10 when reranked, and reranked
BM25 beats an expensively-trained reasoning-tuned dense retriever used alone at 29.1. A
default that returned the 14.8 would be a footgun.

The architectural consequence is the reason the first stage can be cheap:

> **A cheap stage that over-fetches, plus a good reranker, beats an expensive retriever
> alone.**

The first stage only has to get the right passage into the top 40. It does not have to
rank it. That is why the window is wider than any `--limit` anyone types.

### It is not an embedding model

That is the whole difficulty. `Qwen3-Reranker` is a **causal language model**. It emits no
vector. It is asked a yes/no question, and the answer is read from the logits at the final
position:

```
score = softmax([logit(no), logit(yes)])[1]     →  P(yes), in [0, 1]
```

The chat template, the single-token `yes` and `no`, and the softmax over exactly those two
logits are the server's to get right — `llama-server` applies the template the GGUF carries
under `--reranking`. What Centinel sends is the Cohere/Jina request OpenAI never published
an equivalent of:

```text
POST {base_url}/rerank   {"model", "query", "documents": [...], "top_n"}
→                        {"results": [{"index", "relevance_score"}, ...]}
```

Scores come back keyed by `index` and in whatever range the server uses; only their order is
relied on. Every document must be scored exactly once, or the response is refused and the
first stage's order stands.

When the reranker ran in-process, an over-long passage was truncated rather than refused —
a shortened judgement is still a judgement, and nothing here is stored. A server decides that
for itself now.

## `--source` and `--tag`

One filter, applied by each arm inside its own query. The BM25 arm applies it in SQL,
against the workspace database the index attaches. The vector arm applies it in Lance:
every row carries its document's source, tags and exclusion, so the filter runs before
distance and the arm comes back with its full depth in distinct chunks however narrow the
filter is. The placements a hit cites pass the same filter on the way out, so a passage
shared by two documents never names one the filter left out.

A filter is not free under an IVF index: Lance still reads the probed partitions and only
skips the arithmetic for the rows it masks. A search narrowed to one source of thirty reads
about as many bytes as an unfiltered one.

## What a result tells you about itself

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

`method` is **assembled from what ran**, never hard-coded — it is the one field a reader
trusts to know what they are looking at, and a stale literal there is worse than no field.

```
stormwater drainage fee    2 results · bm25→rerank
! keyword search only — no vectors at ~/.centinel/chunks.lance — run `centinel embed` first
```

What it does not say is how much of the corpus the vector arm could see. It used to: a
chunk count beside the vector count, printed as a share. The count was a `COUNT(*)` over
every chunk — 99 s cold at 21.7 million, paid by every search — and the share ignored
`--source` and `--tag`, counted stale vectors, and read as zero whenever the vector arm
failed for any reason. A query that audits the corpus to decorate its result is the wrong
shape. Coverage is a set difference, and `centinel embed --dry-run` computes exactly
that: `indexed`, `already_embedded`, `remaining`, `stale`.

**Always on is not the same as always available.** The rule forbids a *flag* that silently
returns worse results. It does not promise that a machine whose rerank server is down
refuses to search. A missing server degrades the answer and says so, in `no_rerank`; it
never turns a query into an error a reader cannot act on. The same holds for an unbuilt vector table: a corpus is
keyword-searchable long before it is embedded.

## The handle

Every result leads its provenance line with the short blob hash, because anything Centinel
prints, Centinel takes back. `centinel read <hash>` and `centinel open <hash>` accept it by
prefix.

## What it uses

| | |
|---|---|
| **embedder** | `Qwen3-Embedding-4B` Q8_0 GGUF · 2,560-dim · Apache-2.0 · first-party |
| **reranker** | `Qwen3-Reranker-0.6B` Q8_0 GGUF · Apache-2.0 · `ggml-org` conversion |
| **runtime** | the servers `[inference]` names — see [Inference](inference.md) |
| **vectors** | `lancedb` 0.33, no default features |
| **keywords** | SQLite FTS5, bundled |
| **fusion** | ours — a few dozen lines |

`lancedb`'s default features are the S3, GCS, Azure and OSS object stores — every one a
network path out of a machine that nothing is supposed to leave.

## Cost

| | |
|---|---|
| a query | two round trips — the query embedding, then the rerank of 40 passages |

With the models in-process, a cold CLI query measured **11 s**, nearly all of it loading the
reranker's weights; a long-running `serve` paid that once. A server keeps its weights loaded,
so a CLI query and a served one cost the same: what the two requests take on the server, which
for the 0.6B reranker on a GPU is well under a second.

Next: [Ops](ops.md).

## Large corpora

Without an index, the vector arm compares the query with every stored vector. That is
exact, and on a small table it is fast. On a large one it reads the whole table each time:
the field store held 2.7 million passages of 4,096 numbers each, 44 GB, and one query took
107 s on a 3.3 GB VM because none of it stayed in memory.

An index replaces that scan. `[vectors]` in `centinel.toml` says which, and how hard a
query looks:

```toml
[vectors]
index = "ivf_pq"   # "ivf_pq" (default), "ivf_flat", or "none"
probes = 64        # partitions a query reads
refine = 2         # ivf_pq: answers re-checked against the full vectors, times kept
# partitions = 1024  # absent: the square root of the vectors
# sub_vectors = 256  # ivf_pq: bytes per compressed vector; absent: a sixteenth of the width
```

- **`ivf_pq`** sorts the vectors into `partitions` by meaning, and keeps a compressed copy
  of each, `sub_vectors` bytes: 256 bytes at 4,096 dimensions, so the index is about 0.7 GB
  where the vectors are 44 and it stays in memory. A query reads the compressed copies in
  the `probes` nearest partitions, then re-checks the best `refine` × its answers against
  the full vectors on disk, so the order it returns is exact.
- **`ivf_flat`** is the partitions alone, over the full vectors. A query reads every vector
  in the partitions it probes, about `probes / partitions` of the table.
- **`none`** is the scan. It also turns an index that is there off, without dropping it.

Build the index with its own command, once the table holds 4,096 vectors:

```sh
centinel vector-index
```

Nothing else builds one, because a build reads every vector. Every `embed` adds the
vectors appended since to the index without training it again, and until it does they are
scanned beside it, so a new passage is searchable at once. When the table has no index, or
has another than `[vectors]` names, `embed` ends its report saying so and naming the
command.

| To | Change |
|---|---|
| find more of the true nearest | raise `probes`, or `refine` |
| answer faster | lower `probes`, or `refine` |
| a sharper compressed copy, a larger index | raise `sub_vectors` |
| turn the index off | `index = "none"` |

`probes` and `refine` apply to the next search. `index`, `partitions` and `sub_vectors`
apply on the next `centinel vector-index`, which replaces the index there, or drops it for
`none`. Run it again too once the table has grown several times over since the index was trained. A
build reads every vector and trains on as many as the memory budget holds, `[resources]
memory`: it reserves the whole budget and runs alone, so a larger budget trains on more of
the table, up to Lance's own sample. It needs disk for the index beside the vectors, about
0.7 GB for `ivf_pq` and as much again as the vectors for `ivf_flat`.

Partition selection makes retrieval approximate. Compare against the exact answer on
questions you know before trusting a setting:

```sh
centinel search --exact "question"
```

Maintenance runs after ingestion, never on the query path. [Tuning for a
machine](../operate/tuning.md) works out every one of these settings from the machine they
run on.

A reproducible synthetic benchmark reports warm exact/indexed latency, recall@10, and the
build time, for `ivf_pq` or `ivf_flat`:

```sh
cargo run --release --locked -p centinel-core --example vector_bench -- 100000 2560 ivf_pq
```

It uses a temporary store, removes it on completion, and excludes index construction
from query timing. Its synthetic vectors test the storage path, not embedding or
reranking quality; use real corpus questions for an end-to-end evaluation.

Earlier `ivf_flat` measurements in a debug build (20 queries, uniformly random 64-dimensional
vectors) found no speedup at small sizes: at 10,000 rows, exact/indexed latency was
9.52/13.92 ms with 96.5% recall@10; at 100,000 rows, 35.47/37.71 ms with 78.5% recall@10.
Random vectors have no clusters for partitions to find, so recall there is a floor, and a
table that fits in memory scans quickly. The index pays where the table does not fit.
