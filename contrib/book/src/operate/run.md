# The run

```bash
centinel run                      # every source: discover → collect → extract → index → embed
centinel run --source agartha       # one of them
centinel run --limit 50           # bound collection, to try a site before committing an hour
centinel run --skip embed         # stop before the hours-long stage
```

Typing six stages in the right order is a chore that also has to be got right — `index`
before `extract` silently indexes nothing — so the order is written down once, and `run`
is the command you actually use.

## Two phases

```
  per source   discover → collect                      network-bound, per-host paced
  then once    extract → transcribe → index → embed    CPU-bound, model-backed
```

Acquisition is **per source** because politeness is per host, and because a 403 on one
site must not stop the next.

Derivation is **corpus-wide** because `transcribe` and `embed` each build a
multi-gigabyte model. With twenty sources, naive per-source chaining spends more time
loading weights than embedding. It also fixes an ordering hazard for free: `index` runs
after *every* source has extracted, so a chunk that appears in two sources is placed
against both.

## Incremental is inherited, not implemented

Nothing in `run` diffs anything. Every stage already skips work it has done, and none of
them keeps a checkpoint file. The work list is always a subtraction:

| Stage | What it subtracts | Where the answer lives |
|---|---|---|
| `collect` | observed markers from the latest `DiscoveryRun` | the log |
| `extract` | blobs with a derivation of bytes, or an `Underivable` | the log |
| `transcribe` | blobs derived by the transcriber from the audio blobs | the log |
| `index` | placements already written, per address | `centinel.db` |
| `embed` | stored chunk hashes from indexed chunk hashes | `vectors.lance/` |

So a second run does nothing, at every stage, for the same structural reason the first one
was resumable. Kill it at chunk 40,000 and re-run; it starts at 40,001.

That is what makes this the cron command. Twice a day costs one sitemap walk per source
plus whatever actually changed, and a run that found nothing says `nothing new` in one
line.

Because a re-crawled site is about 95% identical text, identical chunks hash identically
and never reach the embedding model twice.

## `--limit` bounds collection, not discovery

Nothing may silently cap a discovery run. A truncated snapshot of a source's address set
looks exactly like a source that shrank, and the archive would record that as a fact. So
`--limit` applies to how much gets fetched, never to how much gets enumerated.

Where an enumeration *does* stop on a ceiling, it says so, and the count is printed as *at
least* n. `valhalla.gov` once printed a checkmark beside 500 addresses against a real 1,625
because that caveat was inferred rather than reported.

## Failure is partial, and it is reported

A source that fails is isolated: its remaining stages are skipped, every other source
still runs, and the report names which broke.

A stage whose model is not installed is **skipped, not failed** — an hour of crawling must
not be thrown away over a download that was never started, and the stage resumes on the
next run once the weights are there.

A corpus-wide stage where some targets failed and others did not is still a failure, and
it keeps the numbers of the calls that worked. Half a corpus extracted is still half a
corpus extracted.

The report carries both a `summary` and an `error`. The summary is the line a person reads
— `1 of 19 failed`. The error is every failure joined, for a machine. Rendering the second
in place of the first shows one source's error as though it were the whole story.

## Reading the numbers

Two kinds of figure, and confusing them records something false:

| | | |
|---|---|---|
| **count** | work *this run* did | two calls **add** — 30 chunks + 30 chunks is 60 |
| **total** | what the store now *holds* | two calls do **not** add — the last answer wins |

`total_chunks` is the size of the whole index. Summing a three-source run's three answers
would report the index as three times its size.

## The stages, individually

`run` performs these in order; each is also its own command, for when you want one:

```bash
centinel discover --source agartha --site https://www.agartha.gov --rps 3
centinel collect  --source agartha --limit 50 --rps 5
centinel extract
centinel transcribe
centinel index
centinel embed
```

`collect` also takes `--match`, a coarse substring filter for exploration —
`--match /assets/` pulls just the documents.

`embed` has two flags worth knowing before you commit hours:

```bash
centinel embed --dry-run       # what would be embedded, without loading a model
centinel embed --limit 100     # sample before committing
```

`--dry-run` creates no table. A plan must leave nothing behind.

Next: [Schedules](schedules.md).
