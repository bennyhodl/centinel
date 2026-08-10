# Models

Everything runs locally. Four roles, all Apache-2.0, all pinned to a repository, a commit
revision and a SHA-256 per file.

```bash
centinel models              # what is installed, what is missing
centinel models pull         # fetch what the pipeline needs
centinel models verify       # re-check digests on disk
centinel models prune        # remove files the registry no longer names
centinel models rm <id>      # remove one
```

`centinel models pull` is the fix named by every error that reports a missing weight, and
it is spelled in exactly one place in the code. It used to be written out at seven call
sites, so a rename would have left six of them naming a command that does not exist.

## The roles

| Role | Default model | Gates |
|---|---|---|
| Embedding | `qwen3-embedding-4b` | search — the vector arm |
| Reranking | `qwen3-reranker-0.6b` | search — the final ordering |
| Transcription | `whisper-large-v3-turbo` | transcription |
| Voice activity | `silero-vad` | transcription |

Readiness is rolled up **per role**, not per model, because the registry carries alternates
and any one installed model fills its role. See
[Models in the registry](../reference/models.md) for every entry and every quantization.

## Why these sizes

The embedder is big and the reranker is small on purpose, and the reason is *where the
cost lands*.

The embedder is paid **once per corpus**, in hours. The reranker is paid **per query**, in
milliseconds. On MTEB English Retrieval, Qwen3-Embedding scores 61.83 at 0.6B, 68.46 at 4B
and 69.44 at 8B. Nearly the whole gain is 0.6B→4B, and 8B buys about a point for roughly
double the embedding time. So the budget goes into the embedder once and into the reranker
freely.

Quantization follows the same logic. Q8_0 over Q4_K_M for the embedder, because
quantization there is amortised over hours of work rather than paid per query.

Licence decided the family. Centinel auto-downloads weights and forks redistribute them,
which rules out EmbeddingGemma (Gemma licence) and Jina's reranker (CC-BY-NC).

## Changing the embedder is a rebuild, not a config edit

The vector table records which model wrote it. A query vector from any other model is
**refused at open**, naming the fix.

This matters more than it sounds like. Vectors from two models live in different spaces
and still return a confident ranked list. There is no symptom — no error, no warning, no
empty result. Just a worse ordering, forever.

Width is guarded by the schema itself: the column is a fixed-size list of exactly `dims`
floats, so a wrong width cannot be written at all. The two registry embedders have
deliberately different widths (2,560 and 1,024) so a swap is loud rather than subtle.

A consequence worth stating plainly: `search` is never *told* which embedder to use. It
asks the table. A reader configured differently would otherwise have its query refused and
quietly fall back to one arm.

## Missing weights degrade; they do not refuse

Reranking is **always on** — meaning there is no flag that silently returns worse results.
That is not a promise that a machine with no reranker weights refuses to search.

Missing weights degrade the ordering and say so, in the `no_rerank` field and in the
terminal header. They never turn a query into an error a reader cannot act on. The same
holds for an unbuilt vector table: a corpus is keyword-searchable long before it is
embedded.

In the pipeline, a stage whose model is missing is **skipped, not failed**. An hour of
crawling must not be thrown away over a download that was never started, and the stage
resumes on the next run once the weights are there.

## Where they live, and what they cost

Under a host cache, laid out as `<root>/<repo>/<revision>/` — the on-disk tree mirrors the
Hugging Face repository, so a path on disk is a path in the repo.

A partial download is a `.part` file, counted toward bytes present and reported as
resumable. `centinel models` is a report and leaves nothing behind: it resolves without
creating.

Disk for the vectors themselves, which is separate from the weights: 397,830 chunks ×
2,560 dimensions × 4 bytes = **3.79 GiB**.

`models` is a `Host` op. Not even the scheduler may fire it, because a multi-gigabyte
download must never ambush a 3am run.

## The runtime

GGUF through `llama.cpp` for the embedder and the reranker, in-process. GGUF through
`whisper.cpp` for transcription, in a **separate binary** — see [Install](../start/install.md)
for why the two cannot be linked together.

Not ONNX. The `onnx-community` exports are decoder graphs carrying a KV cache, and CoreML
refuses tensors with zero elements — which is exactly what an empty cache is. That makes
ONNX permanently CPU-only on Apple Silicon. Measured on the same model both ways: ONNX on
CPU gives 5.5 chunks/sec, `llama.cpp` on Metal gives 18.5.

Because everything runs locally, **output quality varies by machine**. So the model tier
that produced an artifact is part of its provenance — every derivation records the tool,
the version and the tier that made it.

Next: [When something is wrong](troubleshooting.md).
