# Crumbs

A **crumb** is an off-host link **recorded and not followed**.

One Source is one exact host. Following an off-host link automatically is how the walk of
one site becomes a walk of the internet, so the link is not fetched — it is *named*, and
left for you. **The recursion is cut by a person, one time per host rather than once per
page**, and it is fractal: every crumb you promote becomes a Source that walks its own host
and drops its own crumbs.

`investigate` has counted these on a seed page since the registry landed. But the
interesting crumbs are not on a front page — they are on the four hundred agenda pages
behind it, and this is the command that reads them off the whole corpus.

```bash
centinel crumbs                              # what does this corpus point at
centinel crumbs show aca-prod.accela.com     # which pages linked there
centinel crumbs ignore www.facebook.com      # refuse one, for the life of the corpus
centinel crumbs allow www.facebook.com       # take it back
```

---

## The list

```console
$ centinel crumbs

crumbs  236 documents in 3 sources · 0.0s
     host                      links  pages
     www.agarthaconnect.com    2,245    132
     apps.agarthagov.net         659    132
     aca-prod.accela.com         264    132
     www.instagram.com           141    139
     twitter.com                 140    139
     www.facebook.com            140    139
  ✓  www.youtube.com             140    139  already a source
     experience.arcgis.com       132    132
     agartha.gob2g.com           132    132
     agartha.maps.arcgis.com     132    132
     agarthagov.hylandcloud.com  132    132
     utilities.agarthagov.net    132    132
  ✓  www.agartha.gov               8      1  already a source
     valhallagov.formstack.com     7      7
     cityofagartha.govqa.us        1      1

centinel investigate https://www.agarthaconnect.com/CRM
  each crumb you promote becomes a Source that walks its own host and drops its own
  crumbs; each one you refuse is what stops the walk. Neither happens on its own.
```

**This list is scanned, not studied.** Sorted by link count, the systems rise and the noise
sinks: a CRM at the top, a permitting portal and a records portal in the middle, a social
network and a font CDN at the bottom.

**Two counts, and the second is the one that tells you what a host *is*.** One page linking
a host twenty times is a widget in a template. Twenty pages linking it once each is a
system. `aca-prod.accela.com` at 264 links across 132 pages is a link in a shared footer;
`valhallagov.formstack.com` at 7 links across 7 pages is seven pages that each chose it.

**A `✓` row is already answered.** `already a source` means a `[[source]]` block claims that
host, or the log holds an Observation on it — see [three states](#three-states) below.

**The last line is the next command**, filled in for the first host still open. Promotion is
one host at a time and always yours.

`--source <id>` scopes the pass to one source's pages; `--max N` raises the cut-off, which
reports what it dropped rather than truncating quietly.

---

## Where they come from

`collect` scans each page's `<a>` tags **as it stores it**. That is the cheapest moment
there will ever be: the markup is already in memory, its kind is already classified, and
those tags are already being walked one layer down for the documents the page encloses —
where the same-host links become enclosures and the off-host ones used to be dropped on the
floor, uncounted. Now the row is written there, to `crumbs/<source>.jsonl`.

Measured over 5,000 pages of 91 KB — 449 MB of blobs:

| | |
|---|---|
| read every blob | 8.8 s |
| read the ledger | 0.05 s |
| the ledger | 2.3 MB |

**The ledger is derived, and the blobs stay the floor under it.** A row goes missing for
three reasons — the page predates the ledger, a write failed, somebody deleted the file —
and in all three the bytes are still there and immutable. So the pass falls through to the
blobs, counts how many it had to read, and names the one command that stops paying:

```console
    236 pages had no record and were read from blobs — `centinel crumbs --rescan` writes
    them down once
```

That count is on the report rather than in a log line on purpose. A slow answer is a fine
failure mode; a quietly incomplete one is not. `--rescan` re-reads every HTML blob in the
selected sources and writes the ledger again:

```console
$ centinel crumbs --rescan

crumbs  236 documents in 3 sources · 0.2s
     …
    236 pages read from blobs and written down
```

A scanner bug is repaired the same way — delete the file, rescan, and the rebuilt answer is
the same answer. That is why the ledger sits beside `log/` rather than in it.

---

## Three states

Standing is **derived on every read**, never stored as a row and never deleted.

| | Meaning | Where it comes from |
|---|---|---|
| *(blank)* | open — nothing decided, nothing collects it. The only kind that wants a person | the default |
| `ignored` | you refused it | `decisions.jsonl` |
| `already a source` | a Source covers this host, so the promotion happened | the config, or the log |

**`already a source` reads two halves, and either is enough.** A `[[source]]` block with a
`site` is you saying this host is yours to collect — *intent*. An Observation on it is proof
it happened — *evidence*. A host added an hour ago and not yet collected is answered by the
first, because offering it back would ask you to decide something you already decided. A
source collected by hand that no config names is answered by the second.

`enabled = false` still claims the host. Choosing a host and then choosing not to run it is
two decisions, and neither of them was *ask me again*.

**A channel claims no host.** `youtube.com` holds every channel there is, so a source for
one says nothing about a crumb pointing at another. Marking the host answered would hide a
host worth mining; leaving it open costs one `crumbs ignore`.

**Already a source beats already refused.** A host that became a Source is answered by the
corpus, and an old `ignore` on it is stale rather than binding.

---

## Refusing one

Nothing in a page records that a person looked at `facebook.com` and decided it was not a
record. So the ruling is the part that gets **stored**:

```console
$ centinel crumbs ignore www.facebook.com --note "a social account, not a record"

✓ refused www.facebook.com
  a social account, not a record
  recorded at 2026-08-10 21:29 · `centinel crumbs allow www.facebook.com` takes it back
```

Refused hosts drop out of the list, and the list says how many it hid:

```console
    2 refused earlier, hidden — `--all` lists them
```

`--all` puts them back, marked:

```console
     twitter.com               140    139  ignored
     www.facebook.com          140    139  ignored
```

`allow` reverses it, and the record that the refusal was made survives — the file is
append-only and latest-wins:

```console
$ centinel crumbs allow www.facebook.com

✓ allowed www.facebook.com
  replaces ignore on 2026-08-10T21:29:25.895194Z
  recorded at 2026-08-10 21:29 · it will be offered again
```

**Rulings are corpus-wide**, in `decisions.jsonl`, which is why they are not log records.
`log/` is per Source, and *"this host is not a Source"* is not a fact about Agartha: filed
there, `facebook.com` would need refusing once per city, and a Source added next year would
re-offer every host already rejected. That one fault is what would make the list not worth
reading twice.

Nothing removes a crumb, and nothing needs to. Standing is derived on every read, so the
`[[source]]` block existing is what stops a host being a candidate. Deleting rows to record
a decision would put the ledger at odds with the blobs it is built from, and the next
`--rescan` would resurrect them.

---

## Following one back to the page

A crumb that looks wrong opens as the page that dropped it.

```console
$ centinel crumbs show aca-prod.accela.com

aca-prod.accela.com  264 links · 132 pages · 0.2s

  carried by
    https://agartha.gov
      2026-08-07 14:23 · 823a05c9f620
    https://www.agartha.gov/in-memoriam-proclamation
      2026-08-03 17:09 · 099f5d866494
    https://www.agartha.gov/archives-awareness-week-2
      2026-08-04 17:57 · 770bd52b7763
    … and 122 pages more

centinel investigate https://aca-prod.accela.com/AGARTHA/Default.aspx
```

Each carrier prints when the page was observed and **the short blob hash it was read out
of** — anything Centinel prints, Centinel takes back, so `centinel open 823a05c9` puts the
page in front of you. See [Reading a result](../use/read.md).

Ten carriers are named and the rest are counted. A host linked from a thousand pages is a
footer link, and the thousandth address that carries it says nothing the tenth did not — but
the **page count in the header stays whole**, so the cap can never read like a host that was
linked ten times.

---

## Promoting one

There is no `promote` verb, because promotion is two commands you already have:

```bash
centinel investigate https://aca-prod.accela.com/AGARTHA/Default.aspx
centinel source add agartha-permits --site https://aca-prod.accela.com/AGARTHA/
```

In practice it is one command and a keystroke: `investigate` ends by offering to run that
second line for you, with the id and any recognised strategy already filled in. Answer `y`,
or pass `-y` to skip the question. Type the `source add` yourself when you want a different
id than the host suggests — as above, where `agartha-permits` says what the host is for.

Investigate first — a crumb tells you a host is linked, not that anything can enumerate it.
See [Investigate and check](investigate.md).

And once it collects, it drops crumbs of its own. That is the shape of the whole thing: the
corpus grows one deliberate host at a time, and every step is a person deciding.

---

## What is truth here

A crumb is a link parsed out of immutable bytes, so it is **derived** — the same guarantee
that lets `extract` drop an `href` from the derived text. The ruling on it cannot be
rebuilt from anything, so it is **truth**:

```
blobs/            TRUTH     what the world served
log/              TRUTH     what this machine observed
runs/             TRUTH     what this machine attempted
decisions.jsonl   TRUTH     what the operator decided
crumbs/           derived   a link read out of a page
```

See [The store](../internals/store.md).

---

Next: [Sources](sources.md) — writing the block a promoted crumb becomes.
