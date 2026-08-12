# Operator

You are the person who decides what gets collected. This page is the whole job, in the
order you do it, with what each command prints.

```bash
centinel doctor                                # is this machine ready
centinel investigate https://www.agartha.gov     # ask before you commit
centinel source add agartha --site …             # name it
centinel run                                   # collect it
centinel list                                  # see what came back
```

Nothing between those lines is hidden. Every stage prints what it did, and every stage can
be run on its own.

This page is the walk-through. The *Operate it* part takes the same five steps one at a
time and goes deeper into each.

---

## 1. Ask the address what it is

`investigate` fetches a couple of dozen pages and tells you what recognises the host, on
what evidence, and how big the thing behind it is. **Nothing is stored.** It is a question.

```console
$ centinel investigate https://www.agartha.gov

… reading robots.txt for www.agartha.gov
… enumerating with `sitemap`
[0/1] sitemap https://www.agartha.gov/sitemap.xml
[1/7] sitemap https://www.agartha.gov/sitemap.xml?page=6

https://www.agartha.gov  3.6s

  seed
    200 · 148 KiB · html · robots.txt read

  recognised
    ✓ sitemap — sitemap.xml (standard)
    robots.txt    declares one sitemap
    declared      https://www.agartha.gov/sitemap.xml

  size
    ! 500 address(es) across 2 sitemaps   (probe STOPPED, 25 req allowed — there is more)
    https://www.agartha.gov/in-memoriam-proclamation
    https://www.agartha.gov/parkinsons-disease-awareness-month-2
    https://www.agartha.gov/child-abuse-prevention-month-5

  measured
    text          3573 chars, 24 per KB, 0% link text
    markup        231 anchors, 30.6 KiB of <script>
    sitemap       declared

  crumbs
    www.agarthaconnect.com  17 link(s)
    apps.agarthagov.net      4 link(s)
    aca-prod.accela.com      2 link(s)
    cityofagartha.govqa.us   1 link(s)

  warnings
    ! stopped at 500 addresses; the surface is larger than this run captured

  centinel source add agartha --site https://www.agartha.gov/ --strategy=sitemap

✔ Add `agartha` to /Users/you/centinel.toml? · yes
```

Read it block by block.

**`recognised`** names the strategy and prints the evidence it recognised on, not just the
verdict. A strategy keys on a *product* — a sitemap, a Granicus listing, a Hyland OnBase
repository — never on a city, so recognising one collects every city running it. See
[Strategies](../internals/strategies.md).

**`size`** is a bounded probe, and the `!` matters: `500 address(es)` with `probe STOPPED`
means *at least* 500. A count that hit a ceiling always says so, because a truncated
snapshot looks exactly like a source that shrank.

**`measured`** is what the seed page's HTML looks like — how much text per kilobyte, how
much of it is link text. A page that is 0% text and 90% anchors is a navigation menu, and
collecting ten thousand copies of one is the most expensive mistake available here.

**`crumbs`** are off-host links that were recorded and **not followed**. One source is one
exact host. `www.agarthaconnect.com` is a different system with a different strategy, and
promoting it to a source is your call, not the crawler's. That refusal is the whole reason
recursion never runs away here. See [Acquisition](../internals/acquire.md).

**The last line is the command, already filled in — and then the offer to run it.** Enter
accepts, `n` declines and leaves the command on screen, `-y` answers it in advance. It adds
the block and nothing else: collecting is still step 3, and a host already in your config is
not offered again.

Three answers are possible: a strategy with its evidence, a set of crumbs meaning the
system you want lives elsewhere, or nothing — said plainly. All three are useful.

> To ask the narrower question — *what would extraction make of this one document* — use
> `centinel check <url>`. It also stores nothing, and it is the one that catches a page
> whose real content is a PDF hanging off it.

Deeper: [Investigate and check](../operate/investigate.md).

---

## 2. Name the source

Answering `y` above ran exactly this, and typing it is how you name a source you did not
just investigate — or file one under an id of your own.

```console
$ centinel source add agartha --site https://www.agartha.gov/ --strategy=sitemap

… added agartha to /Users/you/centinel.toml

✓ agartha site https://www.agartha.gov/
  written to /Users/you/centinel.toml

  centinel run --source agartha
```

It writes one block into your config and prints the next command:

```toml
[[source]]
id = "agartha"
site = "https://www.agartha.gov/"
strategy = "sitemap"
```

A YouTube channel is the same command with a different key:

```bash
centinel source add agartha-council --channel https://www.youtube.com/@CityofAgartha
```

`site` versus `channel` is the **whole** of the website/YouTube difference. They are peers
that differ only in how they are acquired — which is why there is no `centinel youtube`
verb, and why a third kind would not add one either.

`--strategy` is optional. Omit it and the registry is asked on every run, which is the
right default. Pin one when `investigate` showed you the evidence and you accepted it: a
pinned strategy that later stops recognising its own site still runs and *says so*, rather
than silently falling back to something weaker and collecting a front page.

Also worth knowing here: `--match` to collect only addresses containing a substring,
`--rps` to slow this one host down, `--disabled` to write the block without arming it.
Full set in [Sources](../operate/sources.md).

---

## 3. Run it

One command, six stages, each skipping what it has already done.

```console
$ centinel run --source agartha --limit 8 --skip embed

… agartha · discover
… reading robots.txt for www.agartha.gov
… enumerating with `sitemap`
[0/1] sitemap https://www.agartha.gov/sitemap.xml
[6/7] sitemap https://www.agartha.gov/sitemap.xml?page=1
… 11396 resources discovered (11396 new)

… agartha · collect
[0/8] 0 stored, 0 failed
200     95976 1260ms https://www.agartha.gov/in-memoriam-proclamation
200    305555 1063ms https://www.agartha.gov/sites/default/files/…/20180326_in_memoriam.pdf
[1/8] 1 stored, 0 failed
200     96449 1033ms https://www.agartha.gov/parkinsons-disease-awareness-month-2
200    325798  585ms https://www.agartha.gov/sites/default/files/…/20180401_parkinsons_disease_awareness_month.pdf
[8/8] 8 stored, 0 failed

… extract
[0/15] 0 extracted
html     96258  874ch  6ms https://www.agartha.gov/canadian-chamber-commerce-day
pdf     305555 3615ch 20ms https://www.agartha.gov/sites/default/files/…/20180326_in_memoriam.pdf
[14/15] 14 extracted
… 15 documents extracted

… index
… 36 chunks indexed

… embed
[5/5] done

8 new documents  1 source · 23.8s
  ✓ agartha                 site      23.5s
    discover   11,396 addresses · 11,396 new
    collect    8 acquired · 8 changed · 2.9 MiB · 11,388 left to collect

  ✓ extract    15 documents · 26,770 chars
  ✓ index      15 documents · 36 chunks
    embed      skipped — --skip
```

Four things in that transcript are the design, not the formatting.

**Eight addresses became fifteen documents.** Acquisition follows enclosures: each
proclamation page links a PDF, and the PDF is a document in its own right. The
`--limit 8` bounded the *addresses collected*, not the files that arrived.

**`--limit` never reaches discovery.** The sitemap walk found all 11,396 addresses even
though only 8 were fetched. A DiscoveryRun is a full snapshot; a truncated one would look
exactly like a source that shrank, which corrupts the one signal the snapshots exist to
carry. A cap belongs to the stage that spends the resource it bounds — `--limit` bounds
requests to a host, so it stops at the last stage that makes one. See [The run](../operate/run.md).

**`11,388 left to collect` is the resume state, and it is a subtraction.** No checkpoint
file exists. The work list is always *what the source declares, minus what the log already
records*. So a second run does nothing, a killed run resumes, and `centinel run` in cron is
the intended use.

**Every stage line is a stage you can run alone.** `centinel discover`, `collect`,
`extract`, `transcribe`, `index`, `embed` are the same six ops `run` calls for you.

| Stage | What it does | Deeper |
|---|---|---|
| `discover` | Enumerate every address the source declares | [Strategies](../internals/strategies.md) |
| `collect` | Fetch each address, store raw bytes under their own hash | [Acquisition](../internals/acquire.md) |
| `extract` | Derive text — HTML, PDF, spreadsheet, Word, captions | [Reading a document](../internals/extract.md) |
| `transcribe` | Speech to text for audio with no captions, local Whisper | [Transcription](../internals/transcribe.md) |
| `index` | Cut text into chunks, write them to SQLite FTS5 | [Chunking and the index](../internals/index.md) |
| `embed` | Turn each chunk into a vector, local Qwen3. The expensive one | [Embeddings](../internals/embed.md) |

`--skip embed` is the common flag. It stops before the hours-long stage and leaves a
corpus that is collected, extracted and keyword-searchable but not yet semantically
searchable. A later `centinel embed` picks it up where this left it.

---

## 4. See what came back

```console
$ centinel list

valhalla  22 resources · 22 observations
  ✓ 22 live

agartha  213 resources · 237 observations
  ✓ 212 live   ✗ 1 gone

  ✗ gone    https://www.youtube.com/watch?v=UCLzohJmEgvfJOEd4YJNIHbg
    4 failures since 2026-08-04 00:18 · ERROR: [generic] 'cookies-from-browser=brave'
    is not a valid URL

agartha-permits  in centinel.toml, nothing collected yet

1 source holds nothing yet.
  centinel run --source agartha-permits
```

`213 resources · 237 observations` is the versioning: 213 addresses, seen 237 times,
because 24 of them changed and both versions are kept.

**The rows are your config's sources first, then anything else the store holds.** A source
you added five minutes ago has no directory under `log/` yet, and listing the store alone
would leave it out — which reads as an add that did not work. The two gaps this makes
visible point opposite ways: *in the config, nothing collected* is a run waiting to happen,
and a row that says *not in centinel.toml* is a source `centinel run` will skip. The second
is `centinel source adopt`; see [Sources](../operate/sources.md).

A failure is printed with **the reason and the count**, not as a gap. That is load-bearing:
a WAF 403 and a 404 are the same `Err` in most crawlers and completely different facts
here. *This was deleted* and *this is now blocked* are recorded differently, because
conflating them is how a record quietly corrupts.

`centinel history` shows what each run did, newest first, and `centinel history --failed`
narrows it to what broke. When something is wrong, start at
[When something is wrong](../operate/troubleshooting.md).

---

## 5. Keep it running

```bash
centinel schedule set agartha --cron "0 3 * * *"
centinel serve
```

`serve` fires the configured schedules and serves the read API. Or put `centinel run` in
cron and skip the scheduler — the incremental behaviour is identical either way, because it
comes from the store and not from the runner. See [Schedules](../operate/schedules.md).

---

## What is on disk after all that

```
~/.centinel/
  blobs/          TRUTH     immutable, content-addressed, pooled across sources
  log/            TRUTH     append-only: observations, discovery runs, status, derivations
  current/        derived   a tree that mirrors the URLs
  centinel.db     derived   SQLite metadata + FTS5   — the keyword arm
  vectors.lance/  derived   LanceDB chunk vectors    — the semantic arm
```

Only the first two are evidence. Delete everything else and you lose time, not facts. The
corpus is one directory — you can hand it to somebody with `rsync`.

See [The store](../internals/store.md) and [The record](../internals/record.md).

---

Next: [User](user.md) — how somebody asks this corpus a question, from
an agent, without touching any of the above.
