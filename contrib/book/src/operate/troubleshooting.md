# When something is wrong

Start with `centinel doctor`. It prints the store root it opened, the config file that
named it, the corpus size, which binaries are present, and which pipeline gates are
blocked by missing weights. It names the fix beside each gap.

## Read the readiness report correctly

A missing binary carries a **need**, and the three are not the same:

| Need | Meaning |
|---|---|
| `required` | code calls it and a stage stops |
| `optional` | code calls it and a stage degrades |
| `planned` | nothing calls it yet, and the pipeline that will is not built |

`pdftoppm` and `tesseract` are `planned`. They were once reported as required with zero
call sites between them, so a correctly installed machine was told it was not ready. A
readiness check that is wrong pessimistically is the kind people learn to ignore.

`yt-dlp` is the one dependency that reports **staleness**, because its breakage is
predictable rather than surprising — YouTube changes and it ships releases in emergency
clusters. `doctor` warns at ninety days.

## The corpus looks collected and holds nothing

This is the failure mode to watch for, and it is silent. Every symptom looks like success:
resources found, acquisitions succeeded, liveness `live` on all of them, every address
indexed. The corpus gains hundreds of copies of a navigation menu.

Three real shapes of it:

**The page is a wrapper.** On `tampa.gov`, 915 of 1005 pages held their text in a
JavaScript `var pdfURL`, and the HTML we kept was a print notice. The document was at an
address nothing had fetched. This is what **enclosure** scanning exists for — see
[Reading a document](../internals/extract.md).

**The reader took the whole page.** `hillsclerk.com` enumerates 177 addresses without a
mistake and hands back 23,213 characters of navigation for a page whose content is one
sentence. The fix is the page's own **marked region** — `<main>`, `<article>` — read
before anything guesses.

**The strategy was wrong and confident.** 75 Resources, 75 successful acquisitions, 75
copies of a menu reading "Preview link expired", and not one budget figure. This is why
`investigate` prints the evidence for a recognition rather than the verdict alone.

The check, before you commit an hour:

```bash
centinel investigate https://host/       # who recognises this, and on what evidence
centinel check https://host/some/page    # what would extraction make of this one document
centinel run --limit 50                  # then look at what came back
centinel read <handle>
```

`investigate` and `check` both store nothing.

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

## The store is in two places

If searches come back empty from one directory and full from another, you have two stores.
The root defaults to `~/.centinel` for exactly this reason. `centinel doctor` prints which
root it opened and which config file named it — compare those two lines between the
directories.

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
