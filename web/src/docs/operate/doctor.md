# The machine

Before the pipeline, the host. `centinel doctor` is the check to run first and after every
step — it names the fix beside each gap rather than reporting a verdict you have to
interpret.

```console
$ centinel doctor

ready  /data/agartha · 3 sources
config /Users/you/.centinel/centinel.toml

binaries
  ✓  ffmpeg            7.1.1             decodes audio to 16 kHz WAV for the transcription…
  ✓  yt-dlp            2026.07.04        YouTube acquisition
  ✓  pdftotext         26.08.0           second reader for PDFs the primary makes nothing of
  ✓  pdftoppm          26.08.0           will rasterise PDF pages for OCR — ticket #12
  ✓  tesseract         5.5.0             will OCR scanned documents — ticket #12

inference
  ✓  embed       qwen3-embedding-4b      http://127.0.0.1:8080/v1  search
  ✓  rerank      qwen3-reranker-0.6b     http://127.0.0.1:8081/v1  search
  ✓  transcribe  whisper-large-v3-turbo  http://127.0.0.1:8178/v1  transcription

gates
  ✓ search
  ✓ transcription
```

Four blocks, and each answers a different question.

---

## The first two lines answer *which corpus*

```
ready  /data/agartha · 3 sources
config /Users/you/.centinel/centinel.toml
```

The store root that was opened, and the config file that named it. Read them together:
they are the pair that tells you whether the corpus you are about to collect into is the
corpus you think you are searching.

**The failure they exist to catch is silent.** If a search comes back empty from one
directory and full from another, you have two stores. The root defaulted to `.centinel` in
the *working directory* once, and the result was that every shell got its own corpus —
a separate blob pool, a separate log, a separate index, none of them answering a search
against the others, and none of it visible until a search from one directory up came back
empty. It is in `$HOME` now because a store is a corpus you keep, not an artefact of the
directory you were standing in.

Compare those two lines between the directories before believing anything else. The
resolution order for both is in [Sources](sources.md).

**What is not on this report is how much is in that store.** `doctor` asks the machine, and
it answers at the speed of a `command -v`, because it is the first thing you type when
something is already wrong and often the thing you type before everything else. Counting
the corpus was the one line here that grew with the corpus — a walk of the whole blob pool,
every time, to print a number nobody ran `doctor` to see. `centinel status` counts it, off
the log, by source and content kind, and says what it occupies.

---

## `binaries` — and what a missing one costs

Corpus processing shells out to standalone tools. Serving the web app and remote MCP also requires Node.js 22.13+ for the managed TanStack Start authentication server. A missing binary carries
a **need**, and the three are not the same:

| Need | Meaning |
|---|---|
| `required` | code calls it and a stage stops |
| `optional` | code calls it and a stage degrades |
| `planned` | nothing calls it yet, and the pipeline that will is not built |

`pdftoppm` and `tesseract` are `planned` — that is what *"ticket #12"* in their rows means.
They were once reported as `required` with zero call sites between them, so a correctly
installed machine was told it was not ready. A readiness check that is wrong
pessimistically is the kind people learn to ignore, and one people ignore is worth nothing
on the day it is right.

`yt-dlp` is the one dependency that also reports **staleness**, because its breakage is
predictable rather than surprising: YouTube changes something and it ships releases in
emergency clusters. `doctor` warns once yours is past ninety days. A channel source with a
stale `yt-dlp` does not degrade — it stops.

---

## `inference` — which servers are answering

Centinel runs no model itself; each capability is a server the `[inference]` block names.
`doctor` opens a TCP connection to each — all at once, a few seconds at most — and sends no
request, because a request would cost a model call. A row that is not answering says where
it looked and what to do:

```
  ✗  embed       qwen3-embedding-4b      http://127.0.0.1:8080/v1  search
  ✗  embed  nothing is answering at http://127.0.0.1:8080/v1 (Connection refused) — start a
            server for qwen3-embedding-4b there, or point [inference] at one
```

A key the config names and the environment lacks is reported instead of the connection —
it would have been refused anyway. A config file that does not load at all — which is what
an upgrade over a config with `embed_model` in it looks like — stops every CLI command
before it starts, `doctor` included, with what to write instead: the CLI reads the config to
find the store. Under `--root`, or over HTTP and MCP, `doctor` runs and reports it under the
config line, with every gate shut. See [Inference
servers](../reference/inference.md) for the commands that start each server.

---

## `vectors` — whether search can use what is stored

A server can answer and still be the wrong one. The vector table records the model and the
server that wrote it, and a search refuses query vectors from any other — falling back to
keywords alone. That is the state an upgrade leaves a store in, so `doctor` reads the table's
record (one manifest read, no scan) and compares it with `[inference.embed]`:

```
vectors
  ✗  chunks.lance  qwen3-embedding-4b (run in-process, before [inference])
       [inference.embed] names qwen3-embedding-4b at http://127.0.0.1:8080/v1, so search
       cannot use these vectors. If it serves the same model, `centinel embed --adopt
       --dry-run` measures whether it reproduces them
```

A mismatch shuts the *search* gate. The section is absent on a store with no vectors yet.
See [Inference servers](../reference/inference.md#coming-from-the-in-process-models).

---

## `gates` — the only line that predicts behaviour

```
gates
  ✓ search
  ✓ transcription
```

A gate is a **stage**: *search* needs the embed and rerank servers, *transcription* the
transcription server. Reporting them apart is what lets a crawl-only machine with no
transcription server read as able to search.

**A closed gate does not fail a run. It skips a stage.** An hour of crawling must never be
thrown away over a server that was not started, so `run` checks each stage's server before
the stage, and a run with no embedding server collects, extracts and indexes, reports
`embed` as skipped, and picks it up on the next run once the server is answering. The corpus is keyword-searchable long before it is embedded.

The one thing that closed gate *does* change is what a search can see — a corpus with
chunks and no vectors answers on the keyword arm alone and says so in every result. See
[Search](../internals/search.md).

---

Next: [Investigate and check](investigate.md) — the two questions to ask a host before you
collect it.
