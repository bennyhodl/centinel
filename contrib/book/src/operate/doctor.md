# The machine

Before the pipeline, the host. `centinel doctor` is the check to run first and after every
step — it names the fix beside each gap rather than reporting a verdict you have to
interpret.

```console
$ centinel doctor

ready  /data/agartha · 3 sources
config /Users/you/.centinel/centinel.toml

binaries
  ✓  centinel-whisper                    runs whisper.cpp in its own process, out of llama.c…
  ✓  ffmpeg            7.1.1             decodes audio to 16kHz mono PCM for transcription
  ✓  yt-dlp            2026.07.04        YouTube acquisition
  ✓  pdftotext         26.08.0           second reader for PDFs the primary makes nothing of
  ✓  pdftoppm          26.08.0           will rasterise PDF pages for OCR — ticket #12
  ✓  tesseract         5.5.0             will OCR scanned documents — ticket #12

models
  ✓  qwen3-embedding-4b      q8_0     4.0 GiB  search
  ✓  qwen3-embedding-0.6b    q8_0     610 MiB  search
  ✓  qwen3-reranker-0.6b     q8_0     610 MiB  search
  ✓  whisper-large-v3-turbo  q8_0     834 MiB  transcription
  ✓  whisper-tiny            q5_1    30.7 MiB  transcription
  ✓  silero-vad              v5.1.2   864 KiB  transcription

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

Centinel shells out rather than running a second language runtime. A missing binary carries
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

## `models` — installed weights, by role

Everything runs locally, so the weights are a host fact and `doctor` reports them as one.
The size column is real disk. See [Models](../internals/models.md) for what each role does
and [Models in the registry](../reference/models.md) for every entry and quantization.

```bash
centinel models pull      # fetch what the pipeline needs
```

That is the fix named by every error that reports a missing weight, and it is spelled in
exactly one place in the code.

---

## `gates` — the only line that predicts behaviour

```
gates
  ✓ search
  ✓ transcription
```

A gate is a **role**, not a model. The registry carries alternates — `qwen3-embedding-4b`
and `qwen3-embedding-0.6b` both fill *embedding* — so any one installed model opens the
gate. This is why readiness is rolled up per role: reporting per model would show a machine
with a working embedder as incomplete because it lacked the other one.

**A closed gate does not fail a run. It skips a stage.** An hour of crawling must never be
thrown away over a download that was never started, so a run on a machine with no embedder
collects, extracts and indexes, reports `embed` as skipped, and picks it up on the next run
once the weights are there. The corpus is keyword-searchable long before it is embedded.

The one thing that closed gate *does* change is what a search can see — a corpus with
chunks and no vectors answers on the keyword arm alone and says so in every result. See
[Search](../internals/search.md).

---

Next: [Investigate and check](investigate.md) — the two questions to ask a host before you
collect it.
