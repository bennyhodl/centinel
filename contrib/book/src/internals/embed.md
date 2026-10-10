# Embeddings

The expensive stage. About a day, once, on a 400,000-chunk corpus.

```bash
centinel embed --dry-run          # what would be embedded, without loading a model
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

## The model

**`Qwen3-Embedding-4B`**, Q8_0 GGUF, **2,560 dimensions**, 32K context, Apache-2.0, and
published by Qwen themselves. Run in-process through `llama-cpp-2` — no server, no
sidecar, no second language runtime.

Licence decided the family. Centinel auto-downloads weights and forks redistribute them,
which rules out EmbeddingGemma (Gemma licence) and Jina's reranker (CC-BY-NC).

Size is decided by **where the cost lands**. The embedder is paid once per corpus in
hours; the reranker is paid per query in milliseconds. On MTEB English Retrieval, 0.6B
scores 61.83, 4B scores 68.46 and 8B scores 69.44 — nearly the whole gain is 0.6B→4B, and
8B buys about a point for roughly double the embedding time. So the budget goes into the
embedder once and into the reranker freely.

Q8_0 over Q4_K_M for the same reason: quantization here is amortised over hours of work
rather than paid per query.

## The recipe, and why it is written down

Three things a generic embedding wrapper would get wrong, and **none of them errors**:

1. **Last-token pooling**, not mean pooling.
2. **An instruction prefix on queries only.** A query is wrapped as
   `Instruct: {task}\nQuery:{q}`; a document is embedded bare. The asymmetry is the
   model's — it was trained to treat the relationship as directional.
3. **L2 normalization**, so cosine similarity is a dot product.

Each one produces *plausible* vectors when wrong: slightly worse retrieval, no error
anywhere, no symptom. So the code carries a test that asserts on **semantics** rather than
on shapes.

## Batching is not optional

A batch is **one forward pass over many chunks**: one `llama.cpp` context, one `decode`,
every chunk in it as its own `seq_id`. Two costs collapse into that.

The context and its KV cache are built per **call**, not per text:

| | chunks/sec (M1 Max, Metal) |
|---|---|
| one chunk per call | 6.1 |
| batches of 32 | **18.5** (0.6B) / **3.8** (4B) |

And a single ~300-token chunk leaves a GPU almost entirely idle, which is what packing the
whole group into one pass claims. The table above measured only the first of the two — it
predates the packed decode and has not been re-run against it.

So the batch is the unit of work, not the chunk. A batch that fails as a unit — an
over-long chunk, or a group the machine cannot hold — is retried individually, so one bad
chunk cannot cost the other 31.

How wide is a property of the machine rather than of the corpus. `[defaults] embed_batch`
in the config states it, `--batch N` overrides it for one run, and `auto` — the default —
sizes it from the free memory the backend reports once the weights are loaded, capped at
128. The context is sized to the group in hand, so a batch costs what its chunks actually
need rather than a fixed reservation.

An over-long text is **refused, not truncated.** A silently shortened chunk would be
stored under a `chunk_hash` covering text that was never embedded, which makes the record
lie about what it holds. (The reranker does the opposite, for a reason that is
[explained there](search.md#refused-versus-truncated).)

The whole run goes into a single `spawn_blocking`. Inference would otherwise stall the
async runtime, which matters because an HTTP caller's connection has to survive a
multi-hour run.

## Resumability is a consequence, not a feature

No checkpoint file. The work list is:

```
index chunk hashes  −  stored chunk hashes
```

Kill it at chunk 40,000 and re-run; it starts at 40,001. Lance commits a version per
append, so what landed before the kill is there.

`--dry-run` creates no table. A plan must leave nothing behind.

## When a fragment cannot be read

A Lance data file is written once and never rewritten in place. Storage that drops a
write under load — the field case was a USB drive, 36 of 25,003 files — leaves a file of
the right length with nothing in it. The manifest still lists the fragment, so every full
scan fails, and the first full scan of a run is `embed`'s read of the stored hashes. One
bad file blocks the stage.

`centinel verify` asks the same question one fragment at a time, both columns, and names
the fragments that fail: id, data files, the manifest's row count (an estimate — it can
include rows since deleted) and Lance's error. `--repair` drops them from the manifest in
one commit. Nothing is recovered, because the bytes never landed; nothing needs to be,
because a vector is `chunk_hash → text → vector` and the dropped hashes simply fall back
onto the work list above. The next `embed` re-embeds them.

Two properties of the repair are deliberate. It commits against the version the audit
read, so a compaction that rewrote those fragments in between makes Lance refuse the
commit rather than let a stale id drop a fragment the audit never saw. And it does not
compact or prune — `embed` does that afterwards in its own maintenance, as it always has.

The Lance side of this lives in `VectorTable::audit` and `VectorTable::repair`, beside
`append`, `retain` and `maintain`. The op is thin, and `embed` itself does no auditing:
when its scan fails it says what to run and stops.

## The table

```
vectors.lance/    one table, two columns
                    chunk_hash  Utf8
                    vector      FixedSizeList<Float32, dims>
```

No text, no placements, no source. `centinel.db` holds those, and a second copy goes out
of date the first time the corpus changes. `chunk_hash` is the join both stores already
use.

**The model is a property of the table.** Its id lives in the schema metadata, and a query
vector from any other model is refused at open, naming the fix. Vectors from two models
are in different spaces and still return a confident ranked list — there is no symptom but
a worse ordering.

Width is guarded by the schema itself: the column is a fixed-size list of exactly `dims`
floats, so a wrong width cannot be written at all. The two registry embedders have
different widths (2,560 and 1,024) on purpose.

This also means `search` is never *told* which embedder to use. **It asks the table.** A
reader configured differently would otherwise have its query refused and quietly fall back
to one arm.

**One table, not one per model.** Changing the embedder is already a full re-embed rather
than a config edit, so a second model is a rebuild.

## There is no embedding cache

The specification originally called for one: a durable, portable, append-only file of
vectors beside the static files, on the argument that *"swapping vector backends is a
re-import, not a re-embed."*

That was reversed after measurement. A `.lance` dataset is an ordinary directory — a
manifest, a transaction log, data files. `cp -R` copies it, the copy opens and queries,
and a plain scan reads every vector back out. **Extracting vectors from Lance *is* the
re-import.** Publishing is a directory copy, and so is backup. Lance's transaction log
handles an interrupted run better than truncating a torn record did.

What the cache cost was a second write path and a pipeline stage with its own skip
predicate — and a wrong skip predicate is the defect that has cost this project the most.
So `embed` writes vectors where `search` reads them.

## Cost

| | |
|---|---|
| embed a 400k-chunk corpus | ~1 day, once |
| re-embed after a monthly recrawl | ~5% of that |
| disk, vectors at 2,560-dim | 3.79 GiB per 400k chunks |

A full corpus is 397,830 × 2,560 × 4 bytes.

## Not built yet

**ANN indexing.** With no index, Lance scans flat — exact, and the right answer while the
table is small. An `IVF_PQ` index starts to earn its cost somewhere above roughly 100,000
rows, which is a threshold to measure rather than a number to trust.

**MRL truncation.** Qwen3-Embedding is Matryoshka, so a narrower index is a prefix slice
of a stored vector rather than a re-embed. A reversible decision, deferred until something
measures a need for it.

Next: [Models](models.md).

## Diagnosing slow embedding on DGX Spark

A rate such as 0.2 chunks/sec does not by itself establish GPU use. Embedding now
prints available accelerator backends, or explicitly reports CPU fallback. Run
`nvidia-smi` during inference and use `centinel --verbose embed --limit 200` to inspect
llama.cpp's actual layer placement. The installer discovers CUDA under `CUDA_PATH`
or `/usr/local/cuda*` before choosing its backend.

Spark is Linux ARM64 with a GB10 GPU; the x86_64 CUDA release asset does not apply.
Build from the clone with a compatible CUDA toolkit and a GB10 target, following
[NVIDIA's Spark build guidance](https://build.nvidia.com/spark/llama-cpp/instructions):

```sh
CMAKE_CUDA_ARCHITECTURES=121a-real ./install.sh --build --accel cuda --force
```

Measure batch widths on that machine before selecting a larger batch:

```sh
CMAKE_CUDA_ARCHITECTURES=121a-real cargo run --release --locked -p centinel-core \
  --features cuda --example embed_bench -- --sweep
```

The benchmark compares throughput and vector parity. Keep the same model and
quantization when comparing runs. A failed batch is now split into halves until
healthy groups succeed and bad individual chunks can be reported; cancellation is
checked between retries. This preserves batching when one chunk is invalid or a
whole group exceeds backend capacity. The first failure in each batch is visible
in progress output.
