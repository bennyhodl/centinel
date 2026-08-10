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

**`Qwen3-Reranker-0.6B`**, Q8_0 GGUF, 32K context, Apache-2.0. The weights are a community
conversion by `ggml-org` — the llama.cpp organisation — because Qwen publish GGUF for the
embedder only. Digests pin exactly what is fetched.

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
calls over a shared denominator. Raw logits can be large enough to overflow to infinity,
which yields `NaN` and an order that depends on the sort's tie-breaking.

### Refused versus truncated

Unlike `embed`, an over-long document **is truncated, not refused**.

A passage is one candidate among many, and a shortened judgement is still a judgement,
where refusing would drop a result the first stage chose. Nothing here is stored, so
nothing can lie about what it covers. That is the entire difference: `embed` writes a
record, and a record must not claim to cover text that was never read.

## `--source`

The BM25 arm filters in SQL. The vector arm cannot: Lance carries no source column, and a
chunk has many placements across sources.

So it **over-fetches 5×, then post-filters** in one query for the whole candidate set. It
can still under-fill on a corpus one source dominates — a known limit of the post-filter,
not a bug in it.

## What a result tells you about itself

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

`method` is **assembled from what ran**, never hard-coded — it is the one field a reader
trusts to know what they are looking at, and a stale literal there is worse than no field.
The terminal prints the coverage share whenever it is not 100%.

```
stormwater drainage fee    2 results · bm25→rerank · 397,830 chunks indexed
! keyword search only — no vectors at ~/.centinel/vectors.lance — run `centinel embed` first
```

**Always on is not the same as always available.** The rule forbids a *flag* that silently
returns worse results. It does not promise that a machine with no reranker weights refuses
to search. Missing weights degrade the answer and say so; they never turn a query into an
error a reader cannot act on. The same holds for an unbuilt vector table: a corpus is
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
| **runtime** | `llama-cpp-2` in-process · Metal on macOS, CUDA/Vulkan/ROCm opt-in |
| **vectors** | `lancedb` 0.33, no default features |
| **keywords** | SQLite FTS5, bundled |
| **fusion** | ours — a few dozen lines |

`lancedb`'s default features are the S3, GCS, Azure and OSS object stores — every one a
network path out of a machine that nothing is supposed to leave.

## Cost

| | |
|---|---|
| a query, warm process | ~1 second (the reranker) |
| a query, cold CLI | **11 s measured** with the reranker alone; the embedder adds its own load |

The last row is worth knowing, and the measurement is honest about what it covers: 11.35 s
on the Tampa corpus with no vector table, so only the 0.6B reranker was loaded. A query
that also builds the 4B embedder pays more. `centinel serve` and `centinel mcp` load both
once; a short CLI invocation pays on every query.

Next: [Ops](ops.md).
