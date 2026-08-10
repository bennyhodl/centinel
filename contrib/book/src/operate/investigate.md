# Investigate and check

Two questions, asked before you commit an hour of crawling to a host:

```bash
centinel investigate https://host/          # what is this site, and who recognises it
centinel check https://host/some/page       # what would extraction make of this document
```

**Neither stores anything.** They are questions. `investigate` costs a couple of dozen
requests; `check` costs one, plus whatever it encloses.

They exist because the expensive failure in this project is not a crash. It is a run that
succeeds at everything and collects nothing worth having.

---

## `investigate` — what is this host

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
```

### `recognised` prints evidence, not a verdict

A strategy is asked to recognise a *product* — a sitemap, a Granicus listing, a Hyland
OnBase repository — never a city. Recognising OnBase collects every city running OnBase;
teaching it Agartha collects Agartha. That rule is what makes the registry worth having, and
[Strategies](../internals/strategies.md) is where it is argued.

The evidence lines under the checkmark are printed because a confident wrong recognition is
the worst outcome available. A strategy that matched on a weak signal and enumerated a
front page produces 75 Resources, 75 successful acquisitions, and 75 copies of a menu. The
evidence is how you catch that before it costs an hour, not after.

### `size` is bounded, and says when it stopped

`500 address(es)` carries a `!` and `probe STOPPED, 25 req allowed`. That means **at least**
500. A count that hit a ceiling always says so, because a truncated snapshot is
indistinguishable from a source that shrank — and the archive would record the second as a
fact. `valhalla.gov` once printed a clean checkmark beside 500 addresses against a real
1,625, because the caveat was inferred rather than reported.

`--no-probe` skips the walk entirely: recognition only, two requests.

### `measured` is the navigation-menu detector

```
text          3573 chars, 24 per KB, 0% link text
markup        231 anchors, 30.6 KiB of <script>
```

Read it as a ratio. Text per kilobyte, and how much of the text is inside anchors. A page
that is thin on text and heavy on anchors is a menu, and a strategy pointed at menus
collects thousands of copies of one. 30 KiB of `<script>` against 3.5 KB of text is the
other warning: the content may not be in the HTML at all.

### `crumbs` are recorded and not followed

```
www.agarthaconnect.com  17 link(s)
apps.agarthagov.net      4 link(s)
aca-prod.accela.com      2 link(s)
```

One Source is **one exact host**. `aca-prod.accela.com` is a permitting system with its own
strategy and its own pace; `cityofagartha.govqa.us` is a records portal. Following them
automatically is how a crawl becomes unbounded, and it is also how a corpus stops being
attributable to a government.

So an off-host link is *recorded* as a crumb and the operator promotes it — or does not.
That refusal is the whole reason recursion never runs away here. Each crumb you accept is
its own `source add`, with its own investigation first.

What `investigate` shows here is one page's worth. The interesting crumbs are not on a
front page but on the four hundred agenda pages behind it, and `centinel crumbs` reads
those off the whole corpus — with a ruling you can record once per host. See
[Crumbs](crumbs.md).

### Three possible answers, all useful

| Answer | What to do |
|---|---|
| a strategy, with evidence | read the evidence, then run the printed `source add` |
| crumbs and no strategy | the system you want is on another host — investigate that one |
| nothing, said plainly | there is no lever here yet; record it in field notes |

`--user-agent` and `--timeout-secs` are available here and on `check`. A descriptive
User-Agent measurably reduces WAF 403s.

---

## `check` — what would we make of this document

`investigate` answers *can we enumerate this site*. It does not answer *is the text any
good*. That is a different question and it has its own command.

```console
$ centinel check https://www.agartha.gov/donate-life-month-9

https://www.agartha.gov/donate-life-month-9  2.4s
  https://www.agartha.gov/donate-life-month-9
    html · 93.7 KiB · HTTP 200 · served as text/html; charset=UTF-8
    ✓ marked+htmd 0.5.5  ·  855 of text
      title  Donate Life Month
        read the region marked `main`

    # Donate Life Month

    Date Added

    Sunday, April 1, 2018

    Proclamation File

    ## Use the print buttons in the Preview

    Screenshot of print icons

    To properly print this document, hover your mouse over the document PREVIEW area…
    …  (--print for all of it)

    read    less /var/folders/…/centinel-check-wmIKFE/01-donate-life-month.md
    open    open /var/folders/…/centinel-check-wmIKFE/01-donate-life-month.html

  enclosed https://www.agartha.gov/sites/default/files/proclamations/migrated/20180401_donate_li…
    pdf · 294 KiB · HTTP 200 · served as application/pdf
    ✓ pdf-inspector 0.1.7  ·  2,157 of text
      title  Proclamation of April 2018 as Donate Life Month in Agartha
        1 pages

    **WHEREAS**, one of the most meaningful gifts that a human being can bestow upon another…

    **WHEREAS**, more than 33,000 Americans receive a lifesaving organ transplant every year…

  files are in /var/folders/…/centinel-check-wmIKFE — nothing was stored
```

**That transcript is the single most important thing on this page.** The page we asked
about yields 855 characters, and they say *"Use the print buttons in the Preview."* The
document is the 2,157-character proclamation hanging off it. Collect the address and you
archive a print notice. Collect the enclosure and you archive the record.

This is not an Agartha quirk. On `agartha.gov`, **915 of 1,005 pages** held their text in a
JavaScript `var pdfURL`, and the HTML kept was a print notice. Enclosure scanning is the
answer, and this is the command that shows you whether it fired.

Three lines carry the rest of the answer.

**`✓ marked+htmd 0.5.5`** names the reader that won and, under it, *why*:
`read the region marked main`. The page's own `<main>` or `<article>` is read before
anything guesses. `valhallaclerk.com` hands back 23,213 characters of navigation for a page
whose content is one sentence when nothing looks for the marked region first.

**`855 of text` against `93.7 KiB`** is the ratio to distrust. Kilobytes in, characters out.

**`read` and `open`** are printed with the temp paths already filled in, so you can look at
the extracted Markdown and the original side by side. The last line says it plainly:
*nothing was stored*. Point `check` at a PDF directly and it skips straight to the reader.

See [Reading a document](../internals/extract.md) for the reader ladder and what an
`Underivable` records.

---

## The pre-flight, in order

```bash
centinel doctor                          # is this machine ready
centinel investigate https://host/       # who recognises this, and on what evidence
centinel check https://host/some/page    # what would extraction make of one document
centinel source add … --strategy=…       # the line investigate printed
centinel run --limit 50                  # then look at what actually came back
centinel read <handle>
```

Steps two and three cost seconds and store nothing. Step five is the first one that spends
an hour, and by then you have seen the evidence for the strategy and the text it produces.

Skipping to step five is the single most common way to end up with a corpus that looks
collected and holds nothing: resources found, acquisitions succeeded, liveness `live` on
every one, every address indexed — and hundreds of copies of a navigation menu.

---

Next: [Sources](sources.md) — writing the block, and where it lives.
