# Inference servers

Centinel sends its model work to servers. This page is the commands that start them and
every key in `[inference]`. [Inference](../internals/inference.md) explains why.

## The default: llama-server and whisper.cpp

With no `[inference]` block at all, every capability goes to `http://127.0.0.1:8080/v1`,
which is where `llama-server` listens when started with no port. One `llama-server` serves
one model, so the usual setup is three processes and three ports:

```bash
# Embedding — Qwen3-Embedding wants last-token pooling.
llama-server -hf Qwen/Qwen3-Embedding-4B-GGUF:Q8_0 \
  --embedding --pooling last --ubatch-size 2048 --batch-size 2048 --port 8080

# Reranking.
llama-server -hf ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF --reranking --port 8081

# Transcription — whisper.cpp's server, at the OpenAI path, with VAD.
whisper-server -m ggml-large-v3-turbo-q8_0.bin --port 8178 \
  --inference-path /v1/audio/transcriptions \
  --vad --vad-model ggml-silero-v5.1.2.bin
```

```toml
[inference]
base_url = "http://127.0.0.1:8080/v1"

[inference.embed]
model = "qwen3-embedding-4b"
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"

[inference.rerank]
model = "qwen3-reranker-0.6b"
base_url = "http://127.0.0.1:8081/v1"

[inference.transcribe]
model = "whisper-large-v3-turbo"
base_url = "http://127.0.0.1:8178/v1"
```

GPU offload, threads, context size and the rest are the servers' flags, tuned once for the
machine they run on. A router that puts several models behind one port and picks by
`model` — llama-swap, LocalAI, vLLM — needs only the shared `base_url`.

Run the transcription server with VAD. Whisper fabricates sentences in proportion to the
silence it is fed, and a council recording is mostly silence.

`centinel doctor` opens a connection to each server and says which are answering.

## Every key

`[inference]` — what every capability uses unless its own table says otherwise:

| Key | Default | |
|---|---|---|
| `base_url` | `http://127.0.0.1:8080/v1` | the OpenAI-style root; each request appends its path |
| `api_key_env` | none | the **name** of the variable holding a key, never the key |

`[inference.embed]` — a written table must name `model` and `query_prefix`:

| Key | Default | |
|---|---|---|
| `model` | `qwen3-embedding-4b` | what the server is asked for. A new model is a new vector space |
| `query_prefix` | the Qwen3 instruction | put before every search query. `""` for a symmetric model |
| `batch` | `64` | chunks per request. `embed --batch` overrides it for one run |
| `concurrency` | `4` | requests in flight, 1 to 32 |
| `base_url`, `api_key_env` | `[inference]`'s | |

`[inference.rerank]` and `[inference.transcribe]` — `model`, and optionally `base_url` and
`api_key_env`. Defaults `qwen3-reranker-0.6b` and `whisper-large-v3-turbo`.

The language a recording is transcribed in is `[defaults] lang`, or `transcribe --language`.

## OpenRouter, or any hosted API

```toml
[inference.embed]
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
model = "qwen/qwen3-embedding-8b"
query_prefix = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:"
```

Chunk text goes to the server during `embed`, and each query during `search` — that is
what pointing `base_url` off this machine means, and the config file is the one place it
is said. Nothing falls back to a server the config does not name. `centinel serve start`
hands the background service every variable `[inference]` names.

## Coming from the in-process models

Releases before this one ran the models inside `centinel` and kept their weights in a cache
directory (`~/Library/Caches/centinel/models` on macOS, `~/.cache/centinel/models` on
Linux). Those are the files the commands above load — point `-m` at them rather than
downloading again.

The old `[defaults]` keys are refused with what to write instead: `embed_model`,
`embed_batch` and `transcribe_model` all moved here.

A vector table the in-process embedder wrote records the model and no server. Start a
server for the same model, then:

```bash
centinel embed --adopt --dry-run   # measure, change nothing
centinel embed --adopt             # measure, and take the table over if it matches
```

It embeds a sample of stored chunks on the new server and compares the vectors. Every one
must agree past a cosine of 0.99. If they do not, it says by how much, and what to run —
the server that wrote the table, or a fresh `centinel embed` into a new one. A table
OpenRouter wrote works the same way, and the refusal names OpenRouter.

Transcripts the old worker wrote count as transcripts; nothing is transcribed again.
