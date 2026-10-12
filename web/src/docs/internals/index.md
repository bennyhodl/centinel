# Chunking and the index

```bash
centinel index
```

Cuts derived text into chunks and writes them to `centinel.db` — SQLite metadata plus
FTS5, which is the BM25 arm of search.

## The unit is a chunk, and its identity is its text

A **chunk** is a passage of derived text. Its id is `chunk_hash`: the SHA-256 of the text
itself. Not of the document it came from. Not of the address it was found at.

| | |
|---|---|
| target size | **1,200 characters** (~300 tokens) |
| overlap | **150 characters** |
| minimum | 80 characters |

Everything downstream falls out of that one choice.

The same passage on fifty pages is **one** row in `chunk` and fifty rows in `placement`. A
council's standard notice paragraph is embedded once, not fifty times. And a monthly
recrawl of a site that is about 95% unchanged produces about 95% identical hashes, so
`embed` only ever sees what genuinely changed.

## Chunk geometry is load-bearing far outside chunking

`chunk_hash` hashes the chunk's *text*, and the geometry decides the text.

Change the target or the overlap and you get a wholly different set of hashes. The old
chunks stay in the index and every vector in the table is orphaned — a corpus that took a
day to embed, silently detached from the text it describes.

So the index **records the geometry its hashes were built with, and refuses a change that
is not a rebuild.**

## Placement

Where a chunk sits: which source, which address, which derived blob, which character span,
plus the heading trail, the observation time, and the tool that derived it. That is what
makes a result citable.

**The address is part of a placement's identity, and the derived blob is not enough on its
own.** Two pages can extract to byte-identical text — two proclamations issued the same
day, once the template is stripped. Every rule that treated one derived blob as one
document lost the addresses after the first: the index key collapsed them, and the resume
predicate called them done.

That was 285 of 1005 pages collected, extracted, and absent from every search, each citing
another page's URL.

A chunk with several placements is why a search result carries `also_at` — and why each
entry there carries its own hash. A different address is a different document, with its
own bytes and its own history, so the handle cannot be inferred from the one above it.

## The heading path

Chunks carry the markdown heading trail they sit under. This is why the PDF reader that
produces markdown is primary over the one that produces flat text, and why a document's
[title is written into the text as an `# H1`](extract.md#the-title) rather than recorded
beside it.

Only the text is searched. A heading that is not in the text is a heading no query can
reach.

## The write batch

The rows `index` commits as a unit, and it is **one document** — because that is the unit
the skip predicate subtracts.

A batch is chosen by the skip predicate, not by what makes the writer fastest. Widen it to
span documents and a crash mid-batch leaves placements for a document the predicate will
nonetheless call done: a page collected, extracted, and absent from every search. That is
the exact defect the *per address* rule exists to prevent.

Narrowing it is merely slow. A commit is a WAL checkpoint and an FTS5 flush, and one per
row paid both 450,000 times on a corpus of this size.

## The skip predicate

**Placements already written, per address.** Not derivations, not documents — placements,
per address, for the reason above.

## Why FTS5, and why it stays

The BM25 arm remains on SQLite FTS5 rather than moving to LanceDB's own Tantivy index,
even though LanceDB ships one. The two arms are deliberately independent stores: either
rebuilds without touching the other.

The same reasoning is why LanceDB's built-in RRF reranker goes unused. It can only fuse
arms Lance owns.

## Not built yet

**Transcript-aware chunking** — agenda-aligned spans and per-chunk timestamps, which is
what turns a hit into a timestamped citation into a recording.

Next: [Embeddings](embed.md).
