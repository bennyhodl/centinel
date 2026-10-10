# When something is wrong

Start with `centinel doctor` — it prints the store root it opened, the config file that
named it, which binaries are present, and which pipeline gates are blocked. Reading that
report is [The machine](doctor.md); it also covers the two-stores mistake, where searches
come back empty from one directory and full from another.

The symptoms below are the ones the readiness report cannot see.

## The corpus looks collected and holds nothing

The silent one. Every symptom looks like success — resources found, acquisitions
succeeded, liveness `live` on all of them, every address indexed — and the corpus gains
hundreds of copies of a navigation menu.

[Investigate and check](investigate.md) is the page for this, because the cure is a check
run *before* the hour rather than a diagnosis after it. In short, three real shapes:

**The page is a wrapper.** On `agartha.gov`, 915 of 1,005 pages held their text in a
JavaScript `var pdfURL`, and the HTML kept was a print notice. What **enclosure** scanning
exists for — see [Reading a document](../internals/extract.md).

**The reader took the whole page.** `valhallaclerk.com` enumerates 177 addresses without a
mistake and hands back 23,213 characters of navigation for a page whose content is one
sentence. The fix is the page's own **marked region** — `<main>`, `<article>` — read
before anything guesses.

**The strategy was wrong and confident.** 75 Resources, 75 successful acquisitions, 75
copies of a menu reading "Preview link expired", and not one budget figure. This is why
`investigate` prints the evidence for a recognition rather than the verdict alone.

## A search returns nothing you expected

Work backwards through the pipeline. Each stage can be the answer, and each one reports
its own coverage:

1. **Collected?** `centinel list` — resource counts and liveness per source.
2. **Text derived?** The `extract` report counts unreadable documents and names them.
3. **Indexed?** `centinel embed --dry-run` — `chunks indexed`.
4. **Embedded?** The same report — `already embedded` and `remaining`, beside it.

Step 4 is the one people miss. RRF weights by rank alone, so a corpus with 2,309 vectors
out of 397,830 chunks does not degrade gently — it promotes confident results from a tiny
pool and looks identical to a complete one. The search report does not print that share
(a chunk count on every query cost 99 s at 21.7 million chunks, and the share it fed was
wrong under filters); `no_vectors` tells you when the arm did not run at all, and the dry
run tells you how far along it is.

## A source stopped returning anything

Check liveness. A refusal is recorded as one of four states, and the distinction is
load-bearing:

| Liveness | Meaning | Trigger |
|---|---|---|
| `Live` | fetched successfully | 2xx |
| `Gone` | authoritatively absent | 404, 410 |
| `Blocked` | refused, but **not** evidence of absence | 401, 403, 429, robots denial |
| `Error` | transport or server fault | 5xx, timeout, TLS |

A CloudFront or Akamai 403 would otherwise be indistinguishable from "the site didn't
change". Recording it as `Gone` would log a live page as deleted.

If a whole source turns `Blocked`, slow down. `rps` in `[defaults]` is per host and is
deliberately low. A descriptive `--user-agent` measurably reduces WAF 403s.

## A count looks too round

An enumeration that stopped on a ceiling reports `truncated`, and a truncated count is
printed as *at least* n. If you see a suspiciously round number without that caveat, check
the version — this was once inferred three different ways and none of them worked.

## Extraction found nothing in a PDF

`pdf-inspector` flagging `pages_needing_ocr` is a claim about what the reader could
**decode**, not about what the page **holds**. Reading the first as the second once wrote
off 168 of 490 PDFs that had a text layer all along.

There is a fallback — `pdftotext` — and its job is not to guess again at the same
question. It is the admission that the first tool's silence was never evidence.

A verdict of "nothing could be derived from this" is recorded as an **Underivable**,
carrying the pipeline version that reached it. Bumping that version is how a better
extractor gets another go at what an older one gave up on. `--refresh` re-derives
everything, which is expensive and deliberate.

## Embedding fails on `nul byte found in provided data`

The text of a chunk holds a NUL character. Some PDFs keep their strings as UTF-16 and
were read a byte at a time, so `form` arrived as `\0f\0o\0r\0m`; llama.cpp refuses any
text with a zero in it, and `embed` fails the whole batch the chunk was in. Extraction
now removes NULs before text is stored, so new derivations are clean. Derivations made
before that fix still carry them, and so do the chunks built from them.

Re-derive, rebuild the index, then embed:

```bash
centinel extract --refresh --kind pdf --source agartha
centinel index --rebuild --source agartha
centinel embed
```

`--source` scopes the first two to one source; drop it to do the whole store. The
`index --rebuild` is not optional: incremental indexing only inserts, so the old
NUL-bearing chunks would stay in the index beside the clean ones, and `embed` would keep
finding them. A rebuild clears the source's chunks first, and `embed` then prunes the
vectors whose chunks are gone before it embeds the new ones.

## Embedding fails with a Lance read error

`embed` starts by reading every stored chunk hash back out of `vectors.lance/`, and that
scan touches every fragment. If one data file cannot be read, the scan fails, and so does
`embed` — before it has embedded anything, on one file among thousands of good ones. The
error points you here:

```bash
centinel verify            # read every fragment back; name the ones that fail
centinel verify --repair   # drop them from the table
centinel embed             # re-embeds the dropped chunks with everything else outstanding
```

The repair touches only the table's manifest: the dropped fragments' chunks fall off the
stored set and back onto `embed`'s work list, where they are re-embedded from their text
in `centinel.db`. Nothing is recovered from the bad files because there is nothing in them
to recover.

**Check the storage before you re-embed.** The shape this was built for is a drive that
silently dropped writes under load: 36 of 25,003 data files on one store were the right
length and held nothing. `verify --repair` makes the table usable again; it does not make
the disk honest, and embedding a day's worth of vectors onto the same disk only repeats
the failure. Run `verify` once more after the embed if you have any doubt.

`verify` is operator-only, like every command that changes the store. It commits against
the exact version it audited, so if an `embed` compacted the table between the audit and
the repair the repair is refused and you run it again — it will never drop a fragment it
did not read.

## Things that are safe to delete

Only `blobs/` and `log/` are truth. Everything else rebuilds.

| | Cost to rebuild |
|---|---|
| `current/` | minutes |
| `centinel.db` | minutes |
| `vectors.lance/` | **about a day** on a 400,000-chunk corpus |

Derived is not the same as cheap. Backing up the vectors is `cp -R`; a `.lance` dataset is
an ordinary directory and the copy opens and queries.

Next: [The shape](../internals/shape.md).
