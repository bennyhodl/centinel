# Centinel — Collection strategies

**Status:** a plan, not a specification. Nothing here is built. The central claim rests on
four sites and **has not been falsified yet** — §11 names the ten-minute test that would.

**Evidence:** [`docs/FIELD-NOTES.md`](FIELD-NOTES.md) — four sites, walked by hand.
Every claim below cites an entry there. Nothing here is reasoned from first principles.
**Vocabulary:** [`CONTEXT.md`](../CONTEXT.md). **Source**, **enumerate**, **acquire**,
**Resource**, **DiscoveryRun**, **Note**, **Tool**, **Crumb** are used exactly as written there.
**Answers:** [#37](https://github.com/bennyhodl/centinel/issues/37) *what counts as one site*
and [#36](https://github.com/bennyhodl/centinel/issues/36) *vendor APIs as a Source type*.
**Last updated:** 2026-08-08

---

## 1. What this settles

**The operator supplies a link. Centinel supplies everything else.**

```
centinel source add tampa-agendas https://tampagov.hylandcloud.com/251agendaonline/
  → recognised: Hyland OnBase Agenda Online (product)
  → evidence:   host matches *.hylandcloud.com; path matches /NNNagendaonline/
  → enumerate:  POST the meeting search, read the JSON, derive two PDF addresses per meeting
  → warning:    this server answers HTTP 200 on error
  accept? [y/N]
```

The unit of contribution is a **strategy**, never a site. A pull request that teaches
Centinel to recognise Hyland OnBase collects every city running OnBase. A pull request that
adds Tampa collects Tampa.

This is `FIELD-NOTES.md`'s own rule — *"a finding earns a build only when it generalises
past the site that produced it"* — promoted from a note-taking discipline into a merge
criterion.

---

## 2. The finding everything below rests on

**The variance is in `enumerate`. It is not in `extract`.**

Four sites, judged against the three levers the catalogue uses:

| Entry | `extract` fault | `enumerate` fault |
|---|---|---|
| 1 OpenGov Stories | **none.** Reader list correct, output good. | no sitemap; the address set is in `data-public-url` |
| 2 Hyland OnBase | base64 `data:` URIs — **universal** | POST, token, cookie, and a silent cap at 100 |
| 3 CTTV captions | table cells fuse — **universal** | `__doPostBack` pager over opaque `__VIEWSTATE` |
| 4 Hillsborough Clerk | `octet-stream` → `other` — **universal** | **none.** A directory walk. |

**Zero of four needed a site-specific extractor.** Every extraction fault found was a
framework defect that any site triggers, and each one improves every source already in the
store when it is fixed.

That is the whole argument for where the seam goes. A strategy owns `enumerate`, and it
owns the part of `acquire` that decides *which address form holds the document*. It owns
nothing after that.

---

## 3. The trap this avoids

**There must be no per-site extraction hook.**

Ship one and the table-fusing defect gets worked around in forty site plugins instead of
fixed once. Then the framework fix cannot land, because forty plugins depend on the broken
behaviour. That is the fork cost the plugin system exists to avoid, moved one layer down and
made permanent.

The extraction seam that *does* exist is already correct and already data:
`extract::readers_for` is a table of content kind → ordered readers, and `CONTEXT.md` states
why — *"a second reader for a new kind is now an element in a list rather than a fourth
mechanism."* A new **reader** is a framework contribution, keyed on **content kind**. It is
never keyed on a site.

---

## 4. A strategy is a pair

```rust
pub trait Strategy: Send + Sync {
    /// What did you see, and how sure are you? `None` is a valid, common answer.
    fn recognise(&self, seed: &Fetched) -> Option<Recognition>;

    /// Produce the complete Resource set. The same contract `Source::enumerate` has today.
    fn enumerate<'a>(&'a self, ...) -> BoxFuture<'a, anyhow::Result<Enumeration>>;
}
```

The pairing is the design, not a convenience.

- You cannot add a strategy without teaching it to recognise itself.
- You cannot recognise a shape you cannot then handle.

Without that invariant the registry rots into a set of confident half-answers, and §7 shows
what a confident half-answer costs.

`SiteSource` does not disappear. It becomes the **Source** that holds a strategy, and the
strategy replaces the one hardcoded line — `self.discoverer.discover(&self.site)`.
`Source::method()` already exists to record which one spoke, and `CONTEXT.md` already names
it *"the discriminator that later recovers a Source's kind from the store alone."*

---

## 5. What a strategy keys on

A **product**, a **framework**, a **server default**, or a **standard**. Never a
jurisdiction. Every one of those ships to many cities, which is what makes the work
amortise.

| Entry | Keyed on | Kind | Fingerprint on the seed |
|---|---|---|---|
| 4 (system A) | `sitemap.xml` | standard | `robots.txt` names one |
| 4 (system C) | IIS directory index | server default | `Index of /` markup, no page chrome |
| 3 | ASP.NET WebForms + Telerik RadGrid | framework | `__VIEWSTATE`, `__doPostBack`, `Telerik.Web.UI` |
| 2 | Hyland OnBase Agenda Online | product | `*.hylandcloud.com`, path `/NNNagendaonline/` |
| 1 | OpenGov Stories | product | `stories.opengov.com`, `js-toc-story-link` |

**The two-sighting rule now has teeth.** A strategy merges when it recognises two
deployments. A reviewer can ask a pull request one question — *which two hosts does this
recognise?* — and the answer is checkable. OnBase at one city is a note. OnBase at two
cities is a strategy.

`FIELD-NOTES.md` reached this conclusion twice already and declined to build on it both
times, correctly: *"one site does not earn an adapter"*, and of OnBase, *"a second city on
OnBase would make the seam real. But one sighting is one sighting."*

---

## 6. The registry holds no `match`

The registry is a list. Each element tests itself. Nothing dispatches on a kind.

This is `ContentKind::from_magic` one level up — magic bytes are a list of
*(signature → kind)* where each element answers for itself. A strategy registry is magic
bytes for websites.

The codebase has made this decision three times and `CONTEXT.md` records each one. They are
cited here because a strategy registry is the fourth, and the failure mode is identical:

1. **Source** is a trait, not a `kind` field — *"the alternative is a `kind` field and a
   `match` on it at every stage, which is what the codebase had before the trait had
   implementations — nine sites that a third kind would have had to find."*
2. **ContentKind** is one table, not five — *"adding a kind meant ten edits and the compiler
   asked for none."*
3. **`readers_for`** — *"**the order is data** … A second reader for a new kind is now an
   element in a list rather than a fourth mechanism."*

A strategy also explains itself through **Note**, so a new strategy edits no renderer. That
mechanism already exists and needs nothing.

---

## 7. Recognition must carry evidence

A wrong recognition is silent, and it produces a run that looks perfect. Entry 1 is the
proof and it is worth restating in full, because it is the failure this section exists to
prevent:

> 75 Resources, 75 successful acquisitions, 75 Observations, liveness all `live`. Each
> address is its own Placement, so all 75 index. The corpus gains 75 copies of a navigation
> menu that says "Preview link expired", and not one budget figure.

Five rules follow. Each one is drawn from an entry, not from caution.

**1. A `Recognition` carries what was seen, not only a verdict.** The operator accepts or
rejects on the evidence. This is what `Note` is for.

**2. `None` is a first-class answer.** A registry that always answers is lying. This is the
same admission `CONTEXT.md` makes about readers — *"a fallback is not a second guess at the
same question; it is the admission that the first tool's silence was never evidence."*

**3. "No collection strategy" is a valid, final finding.** Entry 4 established it: HOVER is
a query box behind a server-side CAPTCHA, there is no Resource set behind it, and
*"generating case numbers would invent a corpus rather than observe one."* The registry must
be able to recognise a query box **and refuse it**, which is a strategy that enumerates
nothing on purpose.

**4. A recogniser may answer "wrong address".** *The visible URL is not the fetchable one* —
two sightings, and both seeds handed over were wrappers. So a third valid output is a
**Crumb**: the system you want is on another host. The crumb machinery in `FIELD-NOTES.md`
already covers this and needs no addition here.

**5. What was recognised is written down, and later disagreement warns.** Sites change.
When OnBase ships a new version and its fingerprint stops matching, the operator gets a
warning. They do not get a silent switch to a weaker strategy and an empty corpus. This is
the same shape as the **pipeline version** carried on every `Underivable`: a verdict belongs
to one version and says nothing about the next.

---

## 8. What the operator types

`investigate` is not a separate feature. It is the registry's front door. One module, two
callers:

| Caller | Asks the registry |
|---|---|
| `centinel investigate <url>` | *who recognises this?* — prints the answer and the evidence, then stops |
| `centinel run` | *you — do the work* |

```
centinel investigate https://www.hillsclerk.com/

  seed        https://www.hillsclerk.com/  →  200, 212 KB, html
  recognised  sitemap (standard)
              robots.txt allows everything and names a <sitemapindex>
              182 child sitemaps
  crumbs      hover.hillsclerk.com          8 links
              publicrec.hillsclerk.com      2 links

  centinel source add hillsclerk --site https://www.hillsclerk.com/
```

Three outputs, and the shape of each is settled by §7: a strategy with its evidence; a set
of crumbs; or nothing, said plainly.

Promotion needs no new command. `source add` already does it.

---

## 9. Where a strategy that needs real code goes

Most strategies are ordinary Rust in the registry. A few will not be — a site whose
enumeration needs somebody to read its JavaScript first, written by whoever read it.

**Decide the interface shape now. Decide the linking mechanism later.**

Every strategy is a **pure function over bytes**:

```
(seed bytes, final_url) → { addresses[], declared_total?, notes[] }
```

No network inside the strategy. No store access. Four consequences, and all four are
required rather than pleasant:

1. The **Pacer**, `robots.txt` rules and `HostPolicy` stay with the host. A strategy cannot
   hammer a site, because it never fetches.
2. A strategy cannot write a false record. It returns addresses. The host decides what a
   **Resource** is, and §7's canonicalisation rule stays in one place.
3. It is deterministic, so its test fixture is a blob — and `blobs/` is truth, addressed by
   hash. **Every fixture already exists in the store.**
4. The same signature works in-tree, as a subprocess, and as WASM. Moving between them
   changes no strategy's meaning.

Point 4 is what makes the mechanism a later decision. Ranked, when it is time:

| | Mechanism | Verdict |
|---|---|---|
| 1 | **subprocess over stdio, through `tool.rs`** | **Already built.** Deadline, stall timeout, heartbeat, dies with its caller, never reads our stdin. `yt-dlp`, `ffmpeg`, the whisper worker and `pdf-inspector` all go through it. A strategy is a program that reads bytes on stdin and writes JSON on stdout, in whatever language its author reads JavaScript in. |
| 2 | WASM — `wasmtime`, component model | Stable ABI, sandboxed, any source language. A large dependency and a harder authoring story. The right answer only if strategies must one day be untrusted. |
| 3 | `cdylib` + `libloading` | **No.** Rust has no stable ABI. Version skew is a segfault, not a compile error, and the author must match the compiler version. `abi_stable` and `stabby` make it survivable, not good. |

---

## 10. The strategies the evidence already earns

Ranked by how many of the four entries each one collects.

| | Strategy | Covers | Cost |
|---|---|---|---|
| 1 | `sitemap` — a standard | entry 4 system A | **built.** Wrap the existing `Discoverer`. |
| 2 | `listing` — an open directory index | entry 4 system C, ~6 GB | small. A recursive `<A HREF>` walk. |
| 3 | `index` — the address set is on the page and not in a link | entries 1 **and** 2 | medium. See below. |
| 4 | `none` — a query box, recognised and refused | entry 4 system B | trivial, and it is the point of §7 rule 3 |
| 5 | `sequence` — carried state across requests | entries 2 and 3 | **the fragile one.** Hold it. |

**On strategy 3.** This is the promoted shape at two sightings, and the catalogue already
found the seam: *"the lever is not 'read the links'; it is **a source declares where its
addresses are**"* — an attribute on an anchor for OpenGov, a JSON literal inside a `<script>`
for OnBase. The strategy that recognises the *product* supplies that location. The operator
never types it.

**The discipline that comes with it:** the address set is read by a **parse**, never by a
scan. Entry 2 fault 1 is the proof — a text scan of a script body produced
`.pdf?documentType=`, an address that names no document, on a host where every dead address
answers 200.

**On strategy 5.** It needs a cookie jar, a token read off a page, and opaque state carried
forward. `Source::enumerate` today performs one request shape and cannot express a sequence.
Two sightings promotes it to a *shape*; it does not promote it to a *build*. Hold it until
strategies 1–4 are collecting, and until a third site needs it.

---

## 11. The universal fixes come first

These are not strategies. They belong to no site, they need no config, and each one improves
every source already in the store. They are listed here because building strategies before
them collects more of everything into the same defects.

| | Fix | Reach | Entry |
|---|---|---|---|
| 1 | render `<table>` with cell and row boundaries | **every HTML source in the store** | 3 |
| 2 | strip `data:` URIs from extracted text | every source, every kind | 2 |
| 3 | `application/octet-stream` means *no declaration* — fall back to the inferred type | every file server and open-data portal | 4 |
| 4 | enclosure candidates from parsed attributes only, never a scan of a `<script>` body | every page whose JavaScript builds links | 2 |
| 5 | a redirect to an error path is a **Refusal**, not an Observation | every platform that answers 200 on error | 2 |

Two are confirmed against the code, not only against the notes. `ContentKind::classify`
takes `meta` and `bytes` only, so a `.csv` with no magic bytes reaches `Other` and no reader
claims it. `enclosure::script_targets` pulls quoted strings out of `<script>` bodies, which
is exactly what invented the address in fix 4.

**One warning, from entry 4.** Fix 3 makes things worse before it makes them better: it
converts 2.2 GB of honest `Underivable` into 2.2 GB of record-shaped text that chunks into
meaningless vectors at real embedding cost. *A record set is not a document* sits at one
sighting and is not settled here. Fix 3 should land with a decision about record-shaped
files, or behind it.

---

## 12. The bet, and the test that would kill it

**The bet:** `.gov` is a small number of vendors, repeated across thousands of jurisdictions.

Four sites suggest five strategies. If the ratio holds, one pull request collects a hundred
cities. If every county is bespoke, strategies never amortise, and hand-written config is
the better design after all.

**The test, and it is ten minutes with no code.** `FIELD-NOTES.md` already names it — *"the
other 66 Florida counties. Every one has a clerk, and Florida clerks commonly publish a
comparable `publicrec`-style tree."*

Fetch the front page of two more Florida county clerks. Grep each for the five fingerprints
in §5.

- **Two of three hit a known fingerprint** → the bet holds. Build §13.
- **All three are bespoke** → the bet fails. Stop, and reopen §9 with config in mind.

This test runs before anything in §13. It is the cheapest decision in this file, and it is
the only one that can save a month.

---

## 13. Build order

Run §12 first. Then:

| | Work | Estimate |
|---|---|---|
| 1 | the five universal fixes (§11) | ~2 days, and no design is needed |
| 2 | `Strategy` trait, registry, and `recognise`; port `sitemap` and add `listing` | ~3 days |
| 3 | `centinel investigate` — the registry's front door, and crumbs on the output | ~3 days |
| 4 | check a DiscoveryRun against a declared total | ~1 day, and it makes 1–3 trustworthy |
| 5 | `index` strategy, with OpenGov Stories and OnBase as its two sightings | ~3 days |

Item 4 is small and out of order on purpose. Entry 2 caps at 100 in silence and entry 3
prints *"2606 items in 53 pages"* in its footer — the same defect from both ends. Without
the check, "collected the site" and "collected 4% of it" look identical from a search box,
and every strategy above inherits that blindness.

---

## 14. What this does not settle

Each of these is named rather than answered, and none blocks §13.

1. **Record-shaped documents.** A CSV row and a 174-character fixed-width record are each
   already the unit. Chunking does not apply. One sighting (entry 4), and §11 fix 3 makes it
   urgent rather than optional.
2. **An archive is a container of Artifacts.** `.zip` routes to `anydoc`, which expects one
   document. Closer to `acquire` than to `extract`, and nothing models it. One sighting.
3. **Address canonicalisation.** Entry 2's `DownloadFileBytes` ignores the filename in its
   own path, so one document has unbounded addresses. A **Resource** is an address, so
   identity needs a canonical form before enumeration writes one. One sighting.
4. **Title from structured data.** Three sightings, and it is settled as a *rule* — when a
   source enumerates from structured data, the **Title** comes from that data. It is not
   settled as an *interface*: `Enumeration` carries `Resource`s, and a Resource has no title
   field today.
5. **Whether a Source ever spans hosts.** Entry 4 proved a domain is not a Source. The
   reverse — one system across two hostnames — has not been seen.
