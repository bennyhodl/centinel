# The deeper docs

This book is a guide. It is written from the specifications below, which go considerably
deeper and are the authority wherever the two disagree.

They live in the repository, not in the book, because they are working documents that
change with the code.

## In the repository

| Document | What it holds |
|---|---|
| [`CONTEXT.md`](https://github.com/bennyhodl/centinel/blob/master/CONTEXT.md) | The domain language. Every term, and what getting it wrong would record falsely. The [glossary](glossary.md) here is a summary of it. |
| [`docs/SPEC.md`](https://github.com/bennyhodl/centinel/blob/master/docs/SPEC.md) | The settled specification. Every locked decision with its reasoning and its accepted costs, plus the ones still open. |
| [`docs/ARCHITECTURE.md`](https://github.com/bennyhodl/centinel/blob/master/docs/ARCHITECTURE.md) | How it is built. The store, the domain model, and how one function definition becomes a CLI command, an MCP tool and an HTTP route. |
| [`docs/RETRIEVAL.md`](https://github.com/bennyhodl/centinel/blob/master/docs/RETRIEVAL.md) | How a question becomes a cited passage. Chunking, the two stores, the local embedder and reranker, and what a result tells you about how much of the corpus it could see. |
| [`docs/STRATEGIES.md`](https://github.com/bennyhodl/centinel/blob/master/docs/STRATEGIES.md) | Collection strategies. What a strategy is, what it may key on, and the worked examples the rules came from. |
| [`docs/SCHEDULING.md`](https://github.com/bennyhodl/centinel/blob/master/docs/SCHEDULING.md) | The scheduling specification. Reach, the single lane, the run journal, and what a schedule is allowed to be. |
| [`docs/FIELD-NOTES.md`](https://github.com/bennyhodl/centinel/blob/master/docs/FIELD-NOTES.md) | QA findings from real hosts. Where most of the rules in this book came from. |
| [`docs/research/`](https://github.com/bennyhodl/centinel/tree/master/docs/research) | The evidence underneath. ~3,850 lines, ~450 primary-source citations. |

## The research files

Each one is a survey of primary sources behind a decision this book states as settled.

- `semantic-search.md` — embedders, rerankers, hybrid retrieval, the benchmark numbers
  quoted in [Search](../internals/search.md).
- `pdf-and-ocr.md` — what actually reads a `.gov` PDF, and what a "needs OCR" flag means.
- `youtube-and-transcription.md` — captions, `yt-dlp`, Whisper, and voice activity
  detection.
- `crawling-and-sitemaps.md` — sitemaps, robots, politeness, and the shapes municipal
  sites come in.

## Reading order

**To use it:** you are already done. This book covers it.

**To operate a corpus you care about:** `docs/FIELD-NOTES.md`, because it is a catalogue of
the ways a collection looks successful and holds nothing.

**To change the code:** `CONTEXT.md` first, then `docs/SPEC.md`. The vocabulary is
load-bearing, and most of the specification is a series of arguments about what a word is
allowed to mean.

**To trust it:** `docs/research/`, and the tests. Several of the claims in this book —
the vocabulary-gap example, the embedding recipe, the fallback reader — are asserted as
tests rather than described in comments, precisely because they fail silently when wrong.

## Contributing a strategy

The highest-leverage contribution is a **strategy**, because it keys on a product, a
framework, a server default or a standard, and every one of those ships to many cities.

The bar is two sightings. A reviewer asks one question — *which two hosts does this
recognise?* — and the answer is checkable. See [Strategies](../internals/strategies.md)
and `docs/STRATEGIES.md` §16 for the one-file-per-strategy layout.

---

*MIT licensed. Fork this for your city.*
