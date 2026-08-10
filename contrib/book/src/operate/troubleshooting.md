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
3. **Indexed?** `total_chunks_indexed` in the search report.
4. **Embedded?** `vectors_indexed` in the same report, beside it.

Step 4 is the one people miss. RRF weights by rank alone, so a corpus with 2,309 vectors
out of 397,830 chunks does not degrade gently — it promotes confident results from a tiny
pool and looks identical to a complete one. The terminal prints the share whenever it is
not 100%.

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
