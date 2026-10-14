# Embeddings

The expensive stage. About a day, once, on a 400,000-chunk corpus.

```bash
centinel embed --dry-run          # what would be embedded, without sending anything
centinel embed --limit 100        # sample before committing hours
centinel embed                    # the rest; re-run to resume
```

## The model

**`Qwen3-Embedding-4B`**, Q8_0 GGUF, **2,560 dimensions**, 32K context, Apache-2.0, and
published by Qwen themselves — run by whatever server `[inference.embed]` names.
`llama-server --embedding --pooling last` is the recommended one; see [Inference
servers](../reference/inference.md).

Licence decided the family: forks redistribute what Centinel recommends, which rules out
EmbeddingGemma (Gemma licence) and Jina's reranker (CC-BY-NC).

Size is decided by **where the cost lands**. The embedder is paid once per corpus in
hours; the reranker is paid per query in milliseconds. On MTEB English Retrieval, 0.6B
scores 61.83, 4B scores 68.46 and 8B scores 69.44 — nearly the whole gain is 0.6B→4B, and
8B buys about a point for roughly double the embedding time. So the budget goes into the
embedder once and into the reranker freely.

Q8_0 over Q4_K_M for the same reason: quantization here is amortised over hours of work
rather than paid per query.

### Where it runs

Wherever `base_url` points. On `127.0.0.1` nothing leaves the machine. Pointed at
OpenRouter — `base_url = "https://openrouter.ai/api/v1"`, `api_key_env =
"OPENROUTER_API_KEY"`, `model = "qwen/qwen3-embedding-8b"` — the stage runs a bigger model
than most laptops can, at roughly a cent per million tokens.

Anywhere but this machine, it is never silent. **Chunk text is sent to that server**, the
run names it before the first byte moves, and a search against that corpus embeds its query
there too. Nothing else leaves: blobs, the log and the record stay put. The other place
text can leave is a [classifier run](../use/classify.md).

What the calls cost lands in [Spend](../use/spend.md), where a model on your own server is
priced as the cloud one.

## The recipe, and why it is written down

Three things a generic embedding setup gets wrong for Qwen3-Embedding, and **none of them
errors**:

1. **Last-token pooling**, not mean pooling. The server's: `--pooling last`.
2. **An instruction prefix on queries only.** A query is wrapped as
   `Instruct: {task}\nQuery:{q}`; a document is embedded bare. The asymmetry is the
   model's — it was trained to treat the relationship as directional. No server can say
   which models want this, so `[inference.embed] query_prefix` does, and a written
   `[inference.embed]` table must state it — `""` for a symmetric model.
3. **L2 normalization**, so cosine similarity is a dot product. Done here, on every vector,
   whatever the server sends.

Each one produces *plausible* vectors when wrong: slightly worse retrieval, no error
anywhere, no symptom. An opt-in test — `CENTINEL_TEST_INFERENCE=1`, against the servers
your config names — asserts on **semantics** rather than on shapes: the passage that
answers a query must win while sharing no word with it.

## Requests, not passes

A request carries **`batch` chunks** (64 unless `[inference.embed] batch` or `--batch` says
otherwise), and **`concurrency` requests** are in flight at once (4 unless set). A reader
thread keeps the next windows of text coming off SQLite and a writer task appends what has
come back, so the server waits on neither SQLite nor Lance.

How the server turns a request into passes on its GPU — physical batch, context, flash
attention — is its own tuning, set once on the machine it runs on. That used to be
Centinel's, as `embed_batch = "auto"` sized from the GPU's free memory, and it was a
different number on every machine.

A window the server will not take — a 4xx, a timeout after three attempts, a response that
fails verification — is **split in half** and each half sent again, so one chunk the server
will not take costs a dozen requests rather than sixty-four, and is reported as skipped. A
failure about the server rather than the input is the exception: nothing listening, a
refused key, a width the table does not hold, or 5xx and 429 after three attempts. Every
later request would fail the same way, so the run stops on it and the next run resumes.

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
`append`, `remove`, `set_metadata` and `maintain`. The op is thin, and `embed` itself does no auditing:
when its scan fails it says what to run and stops.

## The table

```
chunks.lance/     one table, one row per chunk placed in a document
                    key          Utf8      chunk_hash ␟ source ␟ resource ␟ derived_sha
                    chunk_hash   Utf8
                    source, resource, derived_sha   Utf8
                    tags         List<Utf8>
                    excluded     Boolean
                    vector       FixedSizeList<Float32, dims>
```

No text: `centinel.db` holds that. The row carries its document and what the classifier
workspace decided about it, so a search's filter runs inside the vector query. The work
list is placements, not chunks, and a placement's vector comes from the cheapest place that
has it — another document's row for the same chunk, and only then the model. A store from
before this layout holds `vectors.lance/`, one row per chunk, which this release refuses
rather than reads: upgrade it with the release before this one (`centinel embed --limit 0`
there, nothing sent to a model), or delete it and embed again. When the workspace changes a
document's tags or exclusion, a sync rewrites those two columns on its rows in place.

**The space is a property of the table.** Its schema metadata records the model and the
server that ran it, and a row or a query vector from any other space is refused, naming the
fix. Vectors from two models are in different spaces and still return a confident ranked
list — there is no symptom but a worse ordering. The server is part of it because the space
is a property of the model *as served*: the same weights at two quantizations, or behind
two servers' pooling, are close and not identical.

Width is guarded by the schema itself: the column is a fixed-size list of exactly `dims`
floats, so a wrong width cannot be written at all. A new table takes its width from the
server, with one short request.

This also means `search` is never *told* which embedder to use. **It asks the table**, and
checks the configured server against it. A server that does not match is named in
`no_vectors`, not queried.

**One table, not one per model.** Changing the embedder is a new space, so a second model is
a rebuild — unless the new server reproduces the old one.

### Moving to another server

```bash
centinel embed --adopt --dry-run   # measure, change nothing
centinel embed --adopt             # measure, and take the table over if it matches
```

`--adopt` takes up to 32 stored vectors whose chunks the index still has, embeds those
chunks on the configured server, and compares. When every pair agrees past a cosine of
**0.99**, it records the new server as the table's own — one manifest commit, no row
touched — and the run carries on into the same table. Otherwise it refuses with the worst
and mean agreement and what to run instead.

The same weights behind a new server land at 0.999 and above. That is how a table written
by an earlier release's in-process embedder moves to `llama-server` running the same Q8_0
GGUF, and how a table embedded through OpenRouter moves to a box that serves the same
model.

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

Next: [Inference](inference.md).

## When embedding is slow

The rate is the server's. A rate such as 0.2 chunks/sec usually means the model is running
on the CPU: check the server's own log for where it put the layers (`llama-server` prints
its offload at start-up), and `nvidia-smi` or Activity Monitor while a run is going. GPU
offload, batch and ubatch sizes, and flash attention are the server's flags.

On Centinel's side, `--batch` and `[inference.embed] concurrency` decide how much work is in
flight. A server with one slot gains nothing from more than one request at a time; one with
several (`llama-server --parallel`) can take as many as it has slots.
