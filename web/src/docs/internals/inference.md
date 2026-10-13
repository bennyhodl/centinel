# Inference

Centinel runs no model itself. Three stages need one, and each is a request to a server the
`[inference]` block of the config file names:

| Capability | Request | Gates |
|---|---|---|
| embed | `POST {base_url}/embeddings` — OpenAI | `embed`, and search's vector arm |
| rerank | `POST {base_url}/rerank` — Cohere/Jina | search's final ordering |
| transcribe | `POST {base_url}/audio/transcriptions` — OpenAI, `verbose_json` | `transcribe` |

What comes back is stored here: vectors in the vector table, transcripts in the blob pool.
The server holds the weights and the tuning that makes them fast on its machine — GPU
layers, batch shape, threads, flash attention, VAD. None of that is a Centinel flag, so the
same binary runs on a laptop and on a GPU box, and a team can share one server.

See [Inference servers](../reference/inference.md) for the commands that start them and
every key in `[inference]`.

## Why it moved out of the binary

It used to run in-process: `llama.cpp` linked into `centinel` for the embedder and the
reranker, and `whisper.cpp` in a second binary, because the two vendor conflicting copies of
`ggml` and could not share one. Each machine needed different tuning — a batch width that
fits its memory, a backend for its GPU — and every one of those became a flag or a build
feature. The release shipped three GPU builds, and a CPU host had to compile two C++
libraries from source.

A server already solves all of that, and every serious one speaks the same API.

## Why rerank is the odd one out

OpenAI publishes no rerank endpoint. The shape Cohere and Jina published is what
llama-server (`--reranking`), vLLM, TEI, Infinity and LocalAI all copied:

```text
POST {base_url}/rerank   {"model", "query", "documents": [...], "top_n"}
→                        {"results": [{"index", "relevance_score"}, ...]}
```

The cross-encoder's recipe — Qwen3-Reranker's chat template, reading `yes` and `no` off
the final logits — is the server's to apply. Only the order of the scores is relied on.

## One failure ends a stage; the rest skip a chunk

Some failures are about the server, and every later request would meet them too: nothing
listening, a refused key, a 404 — the server does not serve that path — a transcription
answer without timed segments, and a server still answering 5xx or 429 after three attempts
a few seconds apart. Those end the stage, rather than skipping a corpus one chunk at a time
for hours; the next run resumes where it stopped.

Others may be about the input. Any other 4xx is not retried; a timeout or a dropped
connection is retried twice; a response that fails verification is not retried. An
embedding batch that fails any of these ways is split in half and each half sent again, so
one bad chunk costs a dozen requests rather than sixty-four, and is reported as skipped.

## A missing server degrades; it does not refuse

Reranking is **always on** — there is no flag that silently returns worse results. That is
not a promise that a machine whose rerank server is down refuses to search.

A search whose embedding or rerank server is not answering runs on the arms it has and says
so, in `no_vectors` or `no_rerank` and in the terminal header. A corpus is
keyword-searchable long before it is embedded.

In the pipeline, a stage whose server is not answering is **skipped, not failed**: `run`
opens a connection to it before the stage starts. An hour of crawling must not be thrown
away over a server that was not started, and the stage resumes on the next run.

## The vector table records its space

Vectors from two models live in different spaces and still return a confident ranked list.
There is no symptom — no error, no warning, no empty result. Just a worse ordering, forever.

So the vector table records the **space** its vectors are in: the model, and the server
that ran it. The server is part of it because the space is a property of the model *as
served* — the same weights at two quantizations, or behind two servers' pooling, are close
and not identical. A query or a row from any other space is refused, and the refusal names
the way forward.

Width is guarded by the schema itself: the column is a fixed-size list of exactly `dims`
floats, so a wrong width cannot be written. A new table takes its width from the server,
with one short request; after that, a server answering at another width ends the run before
anything is written.

`search` is never *told* which space to use. It reads the table and checks the configured
server against it. A server that does not match is named, not queried.

### Moving a table to another server

`centinel embed --adopt` is the one way across without re-embedding. It takes a sample of
stored vectors, sends their chunks to the configured server, and compares what comes back
with what is stored. When every pair agrees past a cosine of 0.99 it records the new server
as the table's own; otherwise it refuses with the figures and with what to run instead.

The same weights behind a new server land at 0.999 and above. A different model, or the
same one at a coarser quantization, lands well under on most chunks.

This is how a table written by the old in-process embedder moves to `llama-server`: the
same Q8_0 GGUF, with last-token pooling, reproduces it.

## Why these models

The defaults are the Qwen3 family, and the embedder is big while the reranker is small on
purpose. The reason is *where the cost lands*.

The embedder is paid **once per corpus**, in hours. The reranker is paid **per query**, in
milliseconds. On MTEB English Retrieval, Qwen3-Embedding scores 61.83 at 0.6B, 68.46 at 4B
and 69.44 at 8B. Nearly the whole gain is 0.6B→4B, and 8B buys about a point for roughly
double the embedding time. So the budget goes into the embedder once and into the reranker
freely. Quantization follows the same logic: Q8_0 over Q4_K_M for the embedder, because it
is amortised over hours of work rather than paid per query.

Qwen3-Embedding is **asymmetric**: a query is wrapped as `Instruct: {task}\nQuery:{q}` and a
document is embedded bare. Embedding queries bare produces plausible vectors and quietly
worse retrieval, and no server can say which models want a prefix. So `[inference.embed]
query_prefix` says it, and a written `[inference.embed]` table must state it — `""` for a
symmetric model such as OpenAI's.

## Provenance

The model that produced an artifact is part of its provenance. A transcript's derivation
records the model as the server was asked for it, and the server's `host:port` where a
quantization used to be. The vector table records its space. The spend ledger records every
embedding request; a model on your own server is priced at what the same weights cost on
OpenRouter, so the [Spend](../use/spend.md) page can say what running it there would cost.

Next: [Search](search.md).
