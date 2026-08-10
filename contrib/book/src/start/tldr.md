# What it does

The short version, in one page.

You name a website or a YouTube channel. Centinel finds every address it declares, fetches
each one, pulls readable text out of whatever came back — HTML, PDF, spreadsheet, Word
document, captions, audio — and makes all of it searchable. It keeps the original bytes
forever, and it keeps every version.

```
centinel source add tampa --site https://www.tampa.gov
centinel run
centinel search "stormwater drainage fee"
```

That is the whole product. Everything else is a detail of one of those three lines.

---

## The pipeline

One run, six stages, in this order:

| Stage | What it does |
|---|---|
| `discover` | Enumerate every address the source declares. A sitemap walk, a playlist listing. |
| `collect` | Fetch each address. Store the raw bytes under their own hash. |
| `extract` | Derive text from those bytes. A different reader per content kind. |
| `transcribe` | Speech to text, for audio with no captions. Local Whisper. |
| `index` | Cut the text into chunks and write them to SQLite FTS5. |
| `embed` | Turn each chunk into a vector. Local Qwen3. The expensive one. |

Every stage skips work it has already done, and none of them keeps a checkpoint file. The
work list is always a subtraction — what the source declares, minus what the log already
records. So a second run does nothing, a killed run resumes, and `centinel run` in cron is
the intended use.

---

## What it keeps

```
~/.centinel/
  blobs/          TRUTH     immutable, content-addressed, pooled across sources
  log/            TRUTH     append-only: observations, discovery runs, status, derivations
  current/        derived   a tree that mirrors the URLs
  centinel.db     derived   SQLite metadata + FTS5   — the keyword arm
  vectors.lance/  derived   LanceDB chunk vectors    — the semantic arm
```

Only the first two are evidence. Delete everything else and you lose time, not facts.
The corpus is one directory. You can hand it to somebody with `rsync`.

---

## What a search does

Two retrievers run against the same corpus and neither is a warm-up.

```
query
  ├─ BM25   (SQLite FTS5)          → top 100    instant, no model
  └─ vector (Qwen3-Embedding-4B)   → top 100    one embed call
        └─ RRF fuse (k=60)         → top 40
              └─ Qwen3-Reranker-0.6B → top n    always on
```

BM25 catches the exact token — a name, a motion number, a dollar figure. The vector arm
closes the vocabulary gap: a water quality report says `PWSName` and `Analyte`, and no
keyword search for *drinking water sampling results* will ever reach it. The reranker then
reads each candidate against the question and reorders. That last step is worth more than
either retriever — reranked BM25 measures more than twice as good as raw BM25 — which is
why there is no flag to turn it off.

Every result carries a **handle**: the short hash of the bytes it came from. Anything
Centinel prints, Centinel takes back. `centinel read <hash>` and `centinel open <hash>`
accept it by prefix, git-style.

---

## The models

All local, all Apache-2.0, all fetched by `centinel models pull`.

| Role | Model | Notes |
|---|---|---|
| Embedding | `Qwen3-Embedding-4B` Q8_0 GGUF | 2,560 dimensions, 32K context |
| Reranking | `Qwen3-Reranker-0.6B` Q8_0 GGUF | a cross-encoder, not an embedder |
| Transcription | `whisper-large-v3-turbo` Q8_0 | near-large accuracy, about 8× the speed |
| Voice activity | `silero-vad` | keeps Whisper from inventing words over silence |

The embedder is big and the reranker is small on purpose. The embedder is paid once per
corpus, in hours. The reranker is paid per query, in milliseconds. So the budget goes into
the embedder once and into the reranker freely.

Inference runs in-process through `llama.cpp` and `whisper.cpp`. There is no server, no
sidecar, and no second language runtime.

---

## Three surfaces, one definition

Centinel is a library first. The CLI, the HTTP server and the MCP server are thin
consumers of it.

```console
$ centinel search "lobbyist meeting log"          # CLI
$ curl -X POST localhost:8787/ops/search -d …     # HTTP: JSON in, JSON out
{"jsonrpc":"2.0","method":"tools/list"}           # MCP: over stdio or over HTTP
```

Each verb is one annotated Rust function. Adding one puts it on all three surfaces with no
central list to update. Agents are clients of the record, never its author — what gets
collected does not depend on what any model thought that day.

---

## What it refuses to do

These are the load-bearing refusals. Each one exists because the alternative silently
records something false.

- **A blocked page is not a deleted page.** A WAF 403 and a 404 are the same `Err` and
  completely different facts.
- **A Resource is an address, not a thing in the world.** The same meeting reachable four
  ways is four Resources. Four honest rows beat one confident wrong one.
- **A strategy keys on a product, never on a city.** Recognising Hyland OnBase collects
  every city running OnBase. Teaching it Tampa collects Tampa.
- **A count that hit a ceiling says so.** An enumeration that stopped early is printed as
  *at least* n, because a truncated snapshot looks exactly like a source that shrank.
- **An over-long chunk is refused, not truncated.** A shortened chunk stored under a hash
  covering text that was never embedded makes the record lie about what it holds.

Next: [Install](install.md).
