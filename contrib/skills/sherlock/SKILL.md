---
name: sherlock
description: Find the organizations tied to a place's government that no page in the corpus links to — NGOs, grant makers, quasi-public bodies, contract vendors — and write them to $CENTINEL_ROOT/prospects/<place>.json as Tips. Use when the operator names a city, county or region and wants candidate sources to add. Do not use to judge a host that is already a source; that is `centinel investigate`.
---

# Sherlock

A **Tip** is a host that an outside index named, recorded with the query that found it, and
not collected.

Three words sit beside each other, and each asks a different question:

| Word | Where it comes from | The question |
|---|---|---|
| **Crumb** | a page we hold links off-host | *should we go there?* |
| **Lead** (`STRATEGIES.md` §17) | a host we hold, read badly | *is what we have any good?* |
| **Tip** | no page of ours points to it | *does this exist at all?* |

Sherlock produces the third. Crumbs walk the corpus outward through links, so a crumb can
only reach what somebody linked. A city does not link the foundation that funds its
programs, the authority that shares its board, or the nonprofit that sues it. Those hosts
are unreachable by any walk, and they hold the records the `.gov` site never publishes.

**A Tip is hearsay, and a Crumb is evidence.** A crumb rebuilds from `blobs/`. A Tip cannot
rebuild from anything — a search provider answers differently tomorrow, and no blob
reproduces its answer. So a Tip carries the query that found it, and the claim stays
attributed.

## What it writes

One file per place, at `$CENTINEL_ROOT/prospects/<place>.json`. Overwrite it each run.

`prospects/` sits under the store root and is **not part of the store.** `store` names truth
and derived; a prospect sheet is neither. No Rust reads it. It is there so it travels with
the corpus it describes when the corpus is copied.

```json
{
  "place": "Tampa, FL",
  "run": {
    "at": "2026-08-09T14:22:11Z",
    "provider": "tavily",
    "root": "/Users/ben/.centinel",
    "seen": 41,
    "tips": 12,
    "known": 9,
    "investigated": false,
    "checked_against": {
      "config": "/Users/ben/.centinel/centinel.toml",
      "source_hosts": 19,
      "crumb_hosts": 74,
      "crumbs_from": ["prospects/tampa.json", "prospects/hillsborough.json"]
    }
  },
  "tips": [
    {
      "host": "www.tampabaycf.org",
      "url": "https://www.tampabaycf.org/",
      "class": "grantmaker",
      "what": "Community foundation. Publishes annual grant awards.",
      "tie": "Names City of Tampa programs as grantees in its award lists.",
      "tie_evidence": "https://www.tampabaycf.org/news-categories/grants",
      "found_by": "tavily: \"Tampa community foundation grants awarded city\"",
      "novel": { "in_sources": false, "in_crumbs": false },
      "investigate": null
    }
  ],
  "known": [
    { "host": "tampagov.hylandcloud.com", "seen_as": "crumb",  "from": "tampa" },
    { "host": "apps.tampagov.net",        "seen_as": "source", "from": "cttv" }
  ]
}
```

### The fields, and what each may hold

- **`host`** — the exact hostname. Not the registrable domain. One Source per exact host is
  the rule the field notes arrived at, and `publicrec.hillsclerk.com` is why.
- **`url`** — the bare origin, `https://<host>/`. **Drop the path.** A strategy bounded by the
  directory it was pointed at collects that directory and stops, and collection is the goal.
  The whole host is the target, so the whole host is the address.

  The one exception is a **shared host**, where the rest of the host belongs to somebody
  else's place. `stories.opengov.com/tampa/` and
  `meetings.boardbook.org/Public/Organization/1448` keep their path, because without it the
  address names a vendor and not a city. Ask *does the rest of this host concern this
  place?* Yes, and the path is a cage. No, and the path is the only thing making the row
  true.
- **`class`** — one of `authority`, `grantmaker`, `filings`, `vendor`, `watchdog`.
- **`what`** — one sentence. What the organization is.
- **`tie`** — one sentence. How it is tied to the government of this place.
- **`tie_evidence`** — the address that shows the tie, or `null`. **This one keeps its full
  path**, and the rule above does not apply to it: it is a page for a person to read, not a
  root for a crawler to start from. A `null` here is a visible weakness on the row, which is
  the point of the field.
- **`found_by`** — the provider and the exact query. This is what makes the claim
  reproducible.
- **`novel`** — see below. Values are `true`, `false`, or `null`.
- **`investigate`** — the `InvestigateReport` verbatim, or `null`. See below.

Nothing else. No score, no rank, no recommendation, no `promote` field of your own writing.

## Novelty, and the rule about `null`

A Tip is worth the operator's time only if it is new. Check both:

1. **Sources** — parse `[[source]]` blocks in `centinel.toml` and take the origin of each
   `site` and `channel`. `centinel list --json` carries source **ids** and no hosts, so the
   config is the only place the hosts are written.
2. **Crumbs** — the union of `crumbs[].host` over every `investigate` report available: the
   `investigate` objects already in `prospects/*.json`, plus any report the operator saved.

A candidate that collides moves to `known` with the reason. It is **dropped from the work
list and not from the file.** A silent cut reads as *"nothing was found there"*, which is
false.

**`in_crumbs` is `null` when no crumb data was read.** Not `false`. Unknown and absent are
different answers, and reporting the first as the second claims a novelty nobody proved.
`run.checked_against` says exactly what was consulted, for the same reason.

## `investigate` is nullable, and that is the point

`null` means **not run**. An empty object would mean *run, and it found nothing* — a
different fact.

- **Default: do not run it.** Sherlock names hosts and stops. It is a list of what exists,
  not a verdict on any of it.
- **`--investigate`: one pass per Tip.** Copy the `InvestigateReport` in verbatim. Never
  re-word it, never reduce it to a score.

The report already carries `promote` — the `source add` line, ready to paste, and
deliberately absent when nothing recognised the host, because suggesting it would be
suggesting a corpus of one front page. **Use that field. Never write your own.**

Either way the file drives a full pass later, which is the shape the operator asked for:

```bash
# investigate every tip, one at a time
jq -r '.tips[].url' "$CENTINEL_ROOT/prospects/tampa.json" | xargs -n1 centinel investigate

# once investigated: the promote lines that survived
jq -r '.tips[].investigate.promote | select(.)' "$CENTINEL_ROOT/prospects/tampa.json"

# tips with no evidence for the tie — read these yourself
jq -r '.tips[] | select(.tie_evidence == null) | .host' "$CENTINEL_ROOT/prospects/tampa.json"
```

`--json` is a global flag and is **already the default when stdout is not a terminal**, so a
redirect needs no flag at all.

## The method

1. **Read what is known.** Parse `centinel.toml` for source hosts. Read `prospects/*.json`
   for crumb hosts. Record the counts in `checked_against`.
2. **Query by class, not by name.** Five templates, one per class, with the place
   substituted. The templates are the asset — the same reason `STRATEGIES.md` §18 keeps the
   walk and not the findings. This is what reaches 67 counties rather than one.
3. **Prove the tie.** For each candidate, find one address that shows the tie and put it in
   `tie_evidence`. No address means `null`, and the row still ships.
4. **Sort, then write.** Order `tips` by how much text the host would add, not by any
   provider's relevance score. Collection is the goal.
5. **Stop.** Write the file. Promote nothing. Add nothing to the config.

### The five classes

Ordered by text gained, which is the order to search in.

1. **`authority`** — CRAs, housing authorities, port and aviation authorities, transit
   agencies, economic development corporations. Own domain, own board, own minutes and
   budgets. Richest by a wide margin.
2. **`grantmaker`** — community foundations, arts councils, CDBG and HUD subrecipients.
   They publish the award lists the city does not.
3. **`filings`** — ProPublica's 990 mirror, USASpending, SAM.gov, the state charity and
   corporate registries. **Different in kind:** one national source, not one per place.
   Name it once and do not repeat it in every sheet.
4. **`vendor`** — contract vendors and their portals. A crumb finds these *only if a page
   links them*. Sherlock is for the rest.
5. **`watchdog`** — chambers of commerce, neighborhood associations and their coalitions,
   university policy centers. Bodies that publish **their own** minutes, rosters and
   positions. **Not news**, however local and however good — see the refusals.

### The inclusion test

> **The money or the mandate crosses, and the host holds the record.** An organization is in
> scope if a public record names it — it takes public funds, holds a public contract, sits on
> a public body, or files a public document about the place — **and** what it publishes is
> that record rather than an account of it.

Both halves are checkable. *"Does good work locally"* fails the first, and without it the
sheet becomes a directory of every charity in the county. A newsroom passes the first and
fails the second, which is what the refusal below is for.

## What Sherlock must refuse

Each of these is a rule an agent otherwise breaks.

- **Never promote, and never edit `centinel.toml`.** The operator promotes. The same rule
  the crumb design already set, for the same reason: every judgment in `FIELD-NOTES.md`
  needed a decision no crawler can make.
- **Never write prose outside `what` and `tie`.** One sentence each. A summary of why a host
  matters is a verdict, and recognition carries evidence, not a verdict.
- **Never copy a provider's summary in as fact.** Record the query in `found_by`. The
  provider said it; we did not see it.
- **Never merge hosts into an organization.** An NGO with a marketing site and a separate
  donation portal is two hosts, and one of them holds nothing.
- **Never take a national body with a local page.** One page is not a source.
- **Never take a news organization.** A newspaper, a broadcaster, a nonprofit newsroom, a
  meeting-coverage site — all out, whatever the quality of the reporting. This corpus
  collects what a government wrote about itself, and a newsroom writes about a government.
  The question is not *is it true* but *whose sentences are these*: a board's minutes are
  the board's sentences, and the article about that meeting is the reporter's. Let the
  second in and every search over the corpus returns somebody's frame mixed with the
  record, with nothing in the text marking which is which.

  The line is the publisher, not the format. A neighborhood association's own minutes are
  in; that association's newsletter *about the council* is out. When a host does both, it
  is out — the archive cannot separate them after collection.
- **Never use the provider's own extraction to judge a host.** `centinel check` and
  `centinel investigate` run the real extractor. A vendor's opinion must not stand where the
  archive expects a measurement.

## Providers

Probe for what is installed; bind to none.

| Provider | What it is for |
|---|---|
| `exa` | similarity — *more hosts like this one*. Best for `grantmaker` and `watchdog`. |
| `tavily` | query, snippet and domain filter. Best for proving a tie. |
| `WebSearch` | the fallback. Works, and finds fewer `authority` hosts. |

**Do not use `firecrawl` here.** Mapping and crawling a known host is what `centinel
investigate` already does, against the real extractor and the real strategy registry.
Record the provider actually used in `run.provider`.
