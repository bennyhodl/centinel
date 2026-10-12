# Models in the registry

Every entry pins a Hugging Face repository, a **commit revision**, and a SHA-256 per file.
The on-disk tree mirrors the repo: `<cache>/<repo>/<revision>/<path>`.

Readiness is rolled up per **role**, not per model — the registry carries alternates, and
any one installed model fills its role.

## Embedding — gates search's vector arm

### `qwen3-embedding-4b` *(default)*

Dense retrieval. 2,560-dim Matryoshka, 32K context.
`Qwen/Qwen3-Embedding-4B-GGUF`, first-party, Apache-2.0.

| Variant | Notes |
|---|---|
| `q8_0` | 8-bit. Near-lossless; **the default**. |
| `q6_k` | 6-bit. Smaller, very close to Q8_0. |
| `q5_k_m` | 5-bit. For a machine that cannot hold Q8_0. |
| `q4_k_m` | 4-bit. The floor. |
| `f16` | Half precision. Unquantized reference. |

### `qwen3-embedding-0.6b`

Dense retrieval, 1,024-dim. Faster, materially weaker, and **a different vector space**.
`Qwen/Qwen3-Embedding-0.6B-GGUF`.

| Variant | Notes |
|---|---|
| `q8_0` | 8-bit. Near-lossless. |
| `f16` | Half precision. Unquantized reference. |

The two embedders have deliberately different widths. Changing embedder is a full re-embed
rather than a config edit, and distinct dimensions are what make that failure loud instead
of silent — the vector column is a fixed-size list of exactly `dims` floats, so a wrong
width cannot be written at all.

## Reranking — gates search's final ordering

### `qwen3-reranker-0.6b`

Second-stage reranking. 32K context.
`ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF`, Apache-2.0.

| Variant | Notes |
|---|---|
| `q8_0` | 8-bit. The only published conversion. |

A community conversion by the llama.cpp organisation, because Qwen publish GGUF for the
embedder only. Digests pin exactly what is fetched.

It emits **no vector** — it is a causal LM scored from two logits. See
[Search](../internals/search.md#it-is-not-an-embedding-model).

## Transcription — gates the transcribe stage

### `whisper-large-v3-turbo` *(default)*

Speech to text. Near-large accuracy at about 8× the speed.
`ggerganov/whisper.cpp`.

| Variant | Notes |
|---|---|
| `q8_0` | 8-bit. Near-lossless; **the default**. |
| `q5_0` | 5-bit. For a machine that cannot hold Q8_0. |
| `f16` | Half precision. Unquantized reference. |

### `whisper-tiny`

39M parameters. A smoke test for the pipeline, not an archive.

| Variant | Notes |
|---|---|
| `q5_1` | 5-bit. 32 MB. |
| `f16` | Half precision. |

## Voice activity — gates the transcribe stage

### `silero-vad`

Voice activity detection. Keeps Whisper from hallucinating over dead air.
`ggml-org/whisper-vad`.

| Variant | Notes |
|---|---|
| `v5.1.2` | 885 KB. The version whisper.cpp documents. |

## Which runtime loads which

The file extension is what tells the two runtimes apart, and it is checked:

| Role | Runtime | Extension |
|---|---|---|
| Embedding, Reranking | `llama.cpp`, via `llama-cpp-2`, in `centinel` | `.gguf` |
| Transcription, Voice activity | `whisper.cpp`, in `centinel-whisper` | `.bin` |

The two **cannot be linked into one binary** — see
[Transcription](../internals/transcribe.md#why-it-is-a-separate-binary).

## Managing them

```bash
centinel models              # what is installed, what is missing
centinel models pull         # fetch what the pipeline needs
centinel models verify       # re-check digests on disk
centinel models prune        # remove files the registry no longer names
centinel models rm <id>
```

`centinel models pull` is the fix named by every error that reports a missing weight, and
it is spelled in exactly one place in the code.

`models` is a `Host` op. Not even the scheduler may fire it — a multi-gigabyte download
must never ambush a 3am run.
