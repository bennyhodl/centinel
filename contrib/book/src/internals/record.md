# The record

The domain model. Nine types, and most of the design is in what each one refuses to
represent.

```
Source  (trait — acquisition varies, nothing downstream does)
  ├─ SiteSource      enumerate: sitemap    id: URL         signal: content hash    (computed)
  ├─ ChannelSource   enumerate: playlist   id: video id    signal: metadata revision
  └─ ApiClient       enumerate: paged query  id: vendor GUID  signal: LastModifiedUtc (asserted)
                     — not implemented; the shape the first two left room for

DiscoveryRun    full snapshot of the Resource set a run observed
Resource        (source, natural_key) — an ADDRESS
ResourceStatus  Live | Gone | Blocked | Error, + since, consecutive_failures, last_checked
Observation     one successful fetch — ALWAYS backed by a Blob
Blob            content-addressed bytes
Derivation      Blob → Blob edge, carrying tool + version + model tier + anchors
Underivable     a derivation attempted that produced nothing — tool + version + reason
ChangeEvent     materialized index, rebuildable from Observations
```

## An Observation always has bytes

There is no failure variant, by construction. A failed fetch appends nothing — it mutates
`ResourceStatus` in place instead.

| Liveness | Meaning | Trigger |
|---|---|---|
| `Live` | fetched successfully | 2xx |
| `Gone` | authoritatively absent | 404, 410 |
| `Blocked` | refused, but **not** evidence of absence | 401, 403, 429, robots denial, YouTube's bot wall |
| `Error` | transport or server fault | 5xx, timeout, TLS |

`Blocked` is the load-bearing one. A CloudFront or Akamai 403 would otherwise be
indistinguishable from "the page didn't change" — measured live against real `.gov` hosts
— and recording it as `Gone` would log a live page as deleted.

The same distinction repeats one level down, in external tools. `yt-dlp` reporting "video
unavailable" is evidence about the video. A missing binary, or a hang that had to be
killed, is evidence about *this machine* and says nothing about the video.

## A Derivation always has bytes too, so `Underivable` exists

`Underivable` is the peer of `ResourceStatus` on the derivation side. Without it, "we
tried and there was nothing to get" is unrecordable.

That matters because every stage computes its work list by subtraction. If the only
recordable outcome were a `Derivation`, the extract predicate could only ever be *a
derivation exists* — which is never true for an audio file, so every one of them would be
read, hashed and re-attempted on every run for the life of the corpus.

**The empty blob recorded as derived text** is that invariant broken, not a thing that
exists. It matters because of *which* record it is: the pipeline version is carried on the
`Underivable`, so a verdict mis-filed as a `Derivation` is beyond the reach of the one
mechanism for revisiting it. The empty blob is therefore excluded from the extract
predicate, and "no bytes" is turned into an `Underivable` **at the write site** rather
than in whichever reader happened to get it wrong — because every reader can get it wrong
the same way.

## Pipeline version

Carried on every `Underivable`. A verdict belongs to one pipeline at one version and says
nothing about the next. Bumping it is how a better extractor gets another go at what an
older one gave up on.

An append-only log cannot un-write what a past run recorded, so this is the only cheap way
back. `--refresh` over the whole corpus is the expensive one.

## What is deliberately not an entity

`Document`, `Transcript` and `Sitemap` are **not** types. Derived artifacts are Blobs
linked by a `Derivation` carrying tool, version and model tier — so "the source changed"
stays mechanically distinguishable from "tesseract was upgraded". A sitemap is a
`DiscoveryRun` snapshot.

## DiscoveryRun is a full snapshot

Not a delta. Resources appearing and vanishing between runs *is* the discovery delta,
computed from two snapshots.

Which is why nothing may silently cap one. A truncated snapshot looks exactly like a
source that shrank, and the archive would record that as a fact. `run --limit` applies to
collection and not to discovery for this reason alone.

Where an enumeration genuinely stops on a ceiling, it says so. **Truncated** is a field on
the enumeration, answered by every Source, and it is the one caveat that changes what the
count *means* rather than qualifying it — so a count is printed as *at least* n wherever
it is true.

That field was inferred three ways before it was reported, and none of them worked. A
shrinking delta cannot fire on a first run, or on a source that genuinely grew past the
cap. A `stopped at` substring in the warning list starts lying the day the wording
changes. The strategy that stopped early is the only thing that ever knew.

## Notes

A **Note** is a line of provenance a Source wants shown, and how it should read. It lets a
report print which sitemaps were walked, or which channel tabs returned nothing, without
the renderer learning what a sitemap or a tab is.

A new adapter explains itself through this and edits no renderer. The same mechanism
carries a strategy's recognition evidence and its warnings.

Next: [Strategies](strategies.md).
