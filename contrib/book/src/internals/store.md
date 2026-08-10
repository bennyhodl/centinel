# The store

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only: observations, discovery runs, status, derivations
  current/<source>/…             DERIVED  a tree that mirrors the URLs
  centinel.db                    DERIVED  SQLite metadata + FTS5   — the BM25 arm
  vectors.lance/                 DERIVED  LanceDB chunk vectors    — the vector arm
```

Only the first two are truth. That is what makes the index disposable and the corpus
something you can hand to somebody with `rsync`.

Blobs are **pooled across sources** — the same PDF on two `.gov` sites stores once. Logs
and trees are **per source**, so a single city's corpus stays separable for handoff.

## Derived is not the same as cheap

Everything derived is rebuildable; only some of it is rebuildable over a coffee.

| | |
|---|---|
| `centinel.db` | minutes |
| `vectors.lance/` | **about a day** on a 400,000-chunk corpus |

Both are safe to delete in the sense that nothing evidentiary is lost, and one of them is
a very expensive thing to delete by accident. Backing up the vectors is `cp -R`.

This distinction cost a whole architecture. The specification originally called for a
separate durable embedding cache — a portable append-only file of vectors, on the argument
that swapping vector backends should be a re-import rather than a re-embed. That was
reversed after measurement: a `.lance` dataset is already an ordinary directory, `cp -R`
copies it, the copy opens and queries, and a plain scan reads every vector back out.
Extracting vectors from Lance *is* the re-import.

What the cache would have cost is a second write path and a pipeline stage with its own
skip predicate — and a wrong skip predicate is the defect this project has paid the most
for. So `embed` writes vectors where `search` reads them.

## Two hashes, because they answer different questions

| | Computed over | Used for |
|---|---|---|
| `blob_sha` | **raw bytes** | archive identity, filename in the blob pool, evidentiary fidelity |
| `fingerprint` | **normalized content** | *did this meaningfully change?* |

A page whose only variation is a rotated CSRF token yields a new `blob_sha` and an
unchanged `fingerprint` — archived faithfully, no change event. Raw-only would produce a
new version every recrawl forever. Normalized-only would destroy the ability to prove what
the server actually served.

> The normalization rules are currently a deliberately naive whitespace collapse, marked
> as a placeholder.

## Head read versus whole read

`get_blob` reads the whole file and verifies it against its address, because this is an
evidentiary archive. `blob_head` reads the first few kilobytes and verifies nothing,
because a partial read cannot be checked against a whole-file digest.

Classification uses the second. Anything shown to a person or written back into the record
uses the first.

## Replay

One Source's log, read once and answerable many times. Every derived view — liveness, the
latest Observation per Resource, what was derived from what — is an in-memory scan over
the records that one disk read produced.

A `Replay` is a **snapshot**. It answers what the log said when it was read, so a caller
that appends and wants to see the append takes a new one.

## The layout is named once

Where each thing lives under the store root is named in the `store` module and nowhere
else. A path spelled out by a caller is a second, unenforced copy of that module's header.

## The root is the identity of the corpus

| | |
|---|---|
| `--root DIR`, or `$CENTINEL_ROOT` | somebody typed a path — an instruction |
| `root = "~/corpora/agartha"` in `centinel.toml` | the standing preference |
| `~/.centinel` | the default |

`store` does not answer *which* store. It is handed a root, and `config` decides it. The
default lives in `$HOME` because it once lived in the working directory, and `centinel
run` from two directories built two corpora that shared no blobs, answered no search
against each other, and looked identical from the inside.

Next: [The record](record.md).
