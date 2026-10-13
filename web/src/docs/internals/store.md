# The store

```
<root>/
  blobs/ab/cd/abcd1234…          TRUTH    immutable, content-addressed, pooled across sources
  log/<source>/YYYY-MM.jsonl     TRUTH    append-only: observations, discovery runs, status, derivations
  investigations/<id>/           RUNTIME  one web investigation: report.json, its request log, and the bytes `check` saved
  crumbs/<source>.jsonl          DERIVED  the off-host links each page dropped, written as it was stored
  summaries/<source>.json        DERIVED  what the log adds up to, keyed by the log's size
  icons/<host>.<ext>, .json      DERIVED  the favicon a host serves, fetched once when the web first asks
  current/<source>/…             DERIVED  a tree that mirrors the URLs
  centinel.db                    DERIVED  SQLite metadata + FTS5   — the BM25 arm
  chunks.lance/                  DERIVED  LanceDB chunk vectors    — the vector arm
  workspace/workspace.db         TRUTH    classifier runs, answers, reviews, a person's overrides
  spend/YYYY-MM.jsonl            TRUTH    every model call, its tokens, and what it cost that day
```

Only the entries marked truth are. Blobs and logs are the evidence; `workspace.db` is what
the classifiers and the people reviewing them decided, which no replay of the log can
recover; and `spend/` is what each paid call cost on the day it was made, which a later price
table would get wrong. That is what makes the index disposable and the corpus something you
can hand to somebody with `rsync`.

`investigations/` is neither: it holds what an administrator's *Investigate an address* found,
filed so the report can be read again or handed to an agent. It sits outside the log, holds
no source or observation, and deleting it loses nothing the corpus has.

Blobs are **pooled across sources** — the same PDF on two `.gov` sites stores once. Logs
and trees are **per source**, so a single city's corpus stays separable for handoff.

## Derived is not the same as cheap

Everything derived is rebuildable; only some of it is rebuildable over a coffee.

| | |
|---|---|
| `centinel.db` | minutes |
| `chunks.lance/` | **about a day** on a 400,000-chunk corpus |

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

## Summary

A replay's counts — addresses, observations, liveness, the hosts the log has observed —
written to `summaries/<source>.json` beside the length of every month file they were read
from. The log is append-only, so a file's length is its version: while the lengths match
the summary *is* the replay, and when they do not the next reader replays the log and
writes the summary again. `source list`, the Sources page and `crumbs` read it; `centinel
list` replays and reports the same number, because both are one fold over the same records.

It is rebuilt by the reader rather than maintained on append because the counts are sets —
an address observed twice is one address — and keeping a set current on every append would
rewrite it once per line, which is quadratic over a collect. Delete `summaries/` and the
next read rebuilds it.

## Icons

The web draws a site's favicon beside every source and host. Each one is fetched the first
time the page asks (`GET /workspace/icons/{host}`) and kept in `icons/<host>.<ext>`, with
`icons/<host>.json` beside it saying where it came from. A host with no icon gets only the
record, "none, checked at T", and is not asked again for seven days.

It is not corpus: no Observation, no log line, and no collection run touches it. Delete
`icons/` and the next request finds each icon again. The icon comes from what the store
already holds when it can (the `<link rel="icon">` in a source's collected front page) and
from one request to the host otherwise. Only a defined source's host, or a host a crumb
names, is ever fetched; no third-party icon service is asked, since it would learn every host
the archive follows.

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
