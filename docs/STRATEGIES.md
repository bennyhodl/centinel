# Centinel — Collection strategies

**Status:** a plan, not a specification. §11's five universal fixes are **built** — branch
`enumeration`, five commits. Nothing else here is. The central claim rests on four sites and
**has not been falsified yet** — §12 names the ten-minute test that would.

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
    /// The name recorded on the DiscoveryRun. `Source::method` returns it.
    fn name(&self) -> &'static str;

    /// What did you see, and how sure are you? `None` is a valid, common answer.
    fn recognise(&self, seed: &Fetched) -> Option<Recognition>;

    /// Produce the complete address set. The contract `Source::enumerate` has today.
    fn enumerate(&self, seed: &Fetched) -> anyhow::Result<Enumerated>;

    /// Did this strategy name **pages**, or did it name **documents**? See below.
    fn addresses_are(&self) -> Addresses { Addresses::Pages }
}
```

The pairing is the design, not a convenience.

- You cannot add a strategy without teaching it to recognise itself.
- You cannot recognise a shape you cannot then handle.

Without that invariant the registry rots into a set of confident half-answers, and §7 shows
what a confident half-answer costs.

**`addresses_are` is the third method, and it earns its place.** `sitemap` names **pages**,
so a page can still hide a PDF inside a viewer and the enclosure scan must run. `onbase`
names **documents**, so nothing is left to find — and running the scan there is exactly what
invented `/251agendaonline/.pdf?documentType=`. Without this method `acquire` would have to
test the strategy's *name*, which is the `match` §6 exists to prevent.

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

| | Strategy | Covers | State |
|---|---|---|---|
| 1 | `sitemap` — a standard | entry 4 system A | **built**, and measured: tampa.gov recognises off `robots.txt`. |
| 2 | `listing` — an open directory index | entry 4 system C, ~6 GB | **built**, and measured: `/Civil/bulkdata/` returns 73 addresses, which is what the entry recorded. |
| 3 | `index` — the address set is on the page and not in a link | entries 1 **and** 2 | not built. Medium, and the most valuable one left. See below. |
| 4 | `none` — a query box, recognised and refused | entry 4 system B | not built. Trivial, and blocked on `investigate` rather than on effort. |
| 5 | `sequence` — carried state across requests | entries 2 and 3 | **held.** The fragile one. |

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

## 11. The universal fixes come first — **built**

These are not strategies. They belong to no site, they need no config, and each one improves
every source already in the store. They are listed here because building strategies before
them collects more of everything into the same defects.

All five are on branch `enumeration`, one commit each, 18 new tests.

| | Fix | Reach | Entry |
|---|---|---|---|
| 1 | render `<table>` with cell and row boundaries | **every HTML source in the store** | 3 |
| 2 | strip `data:` URIs from extracted text | every source, every kind | 2 |
| 3 | `application/octet-stream` means *no declaration* — fall back to the served address | every file server and open-data portal | 4 |
| 4 | a URL a script was still assembling is not an address | every page whose JavaScript builds links | 2 |
| 5 | a redirect to an error path is a **Refusal**, not an Observation | every platform that answers 200 on error | 2 |

**Fix 4 shipped narrower than this file first wrote it, and the difference is instructive.**
The original wording was *"candidates from parsed attributes only, never a scan of a
`<script>` body"*. Implemented literally, that deletes the PDFObject path — `tampa.gov` runs
its viewer with **zero** `<embed>` tags, so a `var pdfURL` literal is the only place 915 of
1005 addresses exist. The rule would have removed 91% of one host's documents to fix one
invented address on another.

So the test is **assembled or whole**, not **script or markup**. A literal with a `+`
immediately either side is one piece of a string the browser builds at run time, and it is
skipped. A whole literal still resolves. A filename must also have a stem, which is what
kills `/251agendaonline/.pdf` at its root.

The lesson generalises past this fix: a lever written from one entry can be true about that
entry and wrong as a rule. **Check a lever against the entry it was not written from.**

Fix 3 also changed a stated invariant rather than working around it. `CONTEXT.md` said
*"nothing that was fetched ever consults it"* of the address, which fix 3 makes false, so
that sentence is now *"a name is the last evidence consulted, never the first."*

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
| 1 | the five universal fixes (§11) | **done.** Branch `enumeration` |
| 2 | `Strategy` trait, `strategies/` registry (§16); port `sitemap`, add `listing` | ~3 days |
| 3 | `centinel investigate` — the registry's front door, and crumbs on the output | ~3 days |
| 4 | check a DiscoveryRun against a declared total | ~1 day, and it makes 1–3 trustworthy |
| 5 | **Leads** — record what nothing recognised, and measure it (§17) | ~2 days |
| 6 | `index` strategy, with OpenGov Stories and OnBase as its two sightings | ~3 days |
| 7 | the `write-a-strategy` skill (§18) | ~1 day, and it must come last |

Item 4 is small and out of order on purpose. Entry 2 caps at 100 in silence and entry 3
prints *"2606 items in 53 pages"* in its footer — the same defect from both ends. Without
the check, "collected the site" and "collected 4% of it" look identical from a search box,
and every strategy above inherits that blindness. Item 5 also wants it: an unreached
declared total is the sharpest measure a Lead can carry.

Item 7 comes last because a skill must document a built thing. A skill written against this
file would teach an interface that does not exist yet, and every strategy written from it
would need rewriting on the day the interface arrives.

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

---

## 15. A worked example — entry 2, end to end

Written out because §4 to §10 are each one decision, and the shape only becomes checkable
when one strategy makes all of them at once. Hyland OnBase is the richest of the four: it
keys on a product, it needs a parse and not a scan, it carries a warning forward, and it
runs into the limit of §9's pure-function rule in a way worth seeing.

```rust
// crates/centinel-core/src/strategies/crawl/onbase.rs
//
// Keyed on a PRODUCT — Hyland OnBase Agenda Online. Not on Tampa. Not on Florida.
// At ONE sighting, so §5 says this does not merge. It is written here as the shape.

pub struct OnBaseAgenda;

impl Strategy for OnBaseAgenda {
    fn name(&self) -> &'static str { "onbase-agenda" }

    fn recognise(&self, seed: &Fetched) -> Option<Recognition> {
        let url  = seed.final_url()?;                 // where the bytes CAME from
        let host = url.host_str()?;

        // Two fingerprints. Both belong to the vendor. Neither belongs to a city.
        if !host.ends_with(".hylandcloud.com")                 { return None }
        if !path_matches(url.path(), "/<digits>agendaonline/") { return None }

        // Confirm on the bytes, not only on the address. A parked domain can match a
        // hostname. Only the running application ships this call.
        if !contains(&seed.bytes, "showSearchResults(new SearchResults(") { return None }

        Some(Recognition {
            strategy: self.name(),
            keyed_on: Keyed::Product("Hyland OnBase Agenda Online"),

            // What the operator ACCEPTS on. A bare verdict is what §7 rule 1 forbids.
            evidence: vec![
                Note::new("host",   format!("{host} is *.hylandcloud.com")),
                Note::new("path",   format!("{} matches /NNNagendaonline/", url.path())),
                Note::new("markup", "the page calls showSearchResults(new SearchResults(…))"),
            ],

            // Entry 2 fault 2, carried FORWARD into every run — not printed once.
            warnings: vec![
                Note::warn("liveness", "this server answers HTTP 200 on its error page"),
            ],
        })
    }

    // A PURE FUNCTION over the seed bytes (§9). It does not fetch. The host already
    // fetched, so the Pacer, robots.txt and HostPolicy stay where they are.
    fn enumerate(&self, seed: &Fetched) -> anyhow::Result<Enumerated> {
        // A PARSE, never a scan. Entry 2 fault 1 is what a scan of this page produces.
        let json  = between(&seed.bytes, "showSearchResults(new SearchResults(", "))")?;
        let found: SearchResults = serde_json::from_str(json)?;

        let mut addresses = vec![];
        for m in &found.meetings {
            // The flags the product itself publishes. An address for a document the
            // source SAYS is absent is a guess, and this host answers 200 to a guess.
            if m.is_agenda_available  { addresses.push(document(m.id, DocType::Agenda))  }
            if m.is_minutes_available { addresses.push(document(m.id, DocType::Minutes)) }
            // doctype 3 is the summary. 20 bytes, every meeting sampled. Never asked for.
        }

        Ok(Enumerated {
            addresses,
            // A number for the check in §13 item 4 to disagree with.
            declared_total: Some(found.meetings.len()),
            notes: vec![
                Note::new("meetings", format!("{} on the landing page", found.meetings.len())),
                // The honest limit of a PURE strategy on this product, said out loud —
                // a corpus holding 61 of 2,600 meetings must not look complete.
                Note::warn("history", "older meetings need the search POST — a token, a \
                     cookie, and a window walk. That is `sequence` (§10), and it is held."),
            ],
        })
    }

    // This strategy names documents, so `acquire` runs no enclosure scan. See §4.
    fn addresses_are(&self) -> Addresses { Addresses::Documents }
}

/// `/Documents/DownloadFileBytes/<anything>.pdf?documentType=N&meetingId=ID`
///
/// The path segment is DECORATIVE — the server ignores it. So one document has unbounded
/// addresses, and a Resource *is* an address. The canonical form uses the meeting id and
/// no free text. (§14 item 3, decided here rather than left open.)
///
/// NOT `DownloadFile` — one word shorter, 1,448 bytes of HTML, and it needs a publishId
/// the search result never carries. A decoy, and entry 2 recorded the mistake.
fn document(meeting: u64, doc: DocType) -> String {
    format!("/Documents/DownloadFileBytes/{meeting}.pdf?documentType={}&meetingId={meeting}",
            doc as u8)
}
```

**What it collects, with the city never named:** 61 meetings, up to 122 PDFs at ~280 KB
each. Extraction is already excellent — 123,172 characters out of one minutes document,
Readability first try, headings intact.

**A product can need two strategies, at different purity levels.** This is new, and §10 did
not say it. The `index` half above is pure and buildable now. The full back catalogue needs
the search POST, and a POST is not a pure function over a seed. So OnBase is not one
strategy held until `sequence` exists; it is a buildable strategy that **reports its own
ceiling** and a held one that would lift it. That is a better answer than waiting, and it is
only safe because item 4 makes the ceiling visible.

### Where a source branches

```
  centinel investigate https://tampagov.hylandcloud.com/251agendaonline/
                                │
                    Fetcher::get   ← paced, robots.txt read. The host fetches, never a strategy.
                                │
                    registry.recognise(&seed)
                    a LIST. Each element answers for itself. No `match`. (§6)
        ┌───────────────┬───────────┴────────┬──────────────────┐
     a strategy      a refusal          wrong address         None
        │           (a query box)         (a crumb)             │
        │                │                    │                 │
   evidence +      "no collection      "the system you     a LEAD (§17):
   warnings +       strategy, and       want is on         recorded, measured,
   the `source      that is the         <other host>"      and collected by
    add` line        answer" (§7.3)      (§7.4)             sitemap anyway
```

Then the pipeline. Three of the four stages change nothing at all.

```rust
// ── source add ──────────────────────────────────────────────────────────────
[[source]]
id       = "tampa-agendas"
site     = "https://tampagov.hylandcloud.com/251agendaonline/"
strategy = "onbase-agenda"     # written down, so later disagreement can WARN (§7.5)


// ── sources::from_config — the one `match` on Acquisition. One arm gains one line.
Acquisition::Site(url) => {
    // A NAMED strategy resolves now. An unnamed one cannot: recognition needs a seed,
    // and `from_config` never fetches. So the field is an Option and §7's promise —
    // the operator accepted this one — is what `Some` means.
    let named = cfg.strategy.as_deref().map(registry::by_name).transpose()?;
    Box::new(SiteSource::new(id, url, policy, limits, named)?)
}


// ── discover → SiteSource::enumerate ────────────────────────────────────────
struct SiteSource {
    named:  Option<&'static StrategyDef>,   // what the config declared
    spoke:  OnceLock<&'static StrategyDef>, // what actually ran. `method()` reads this.
    …
}

fn enumerate(&self, progress) -> Enumeration {
    let seed = self.fetch_reporting(&self.site, false, progress).await?;

    let strategy = match self.named {
        // §7 rule 5. Sites change. OnBase ships a new version, the fingerprint stops
        // matching, and the operator is TOLD. They do not get a silent switch to a
        // weaker strategy and an empty corpus.
        Some(s) => {
            if s.recognise(&seed).is_none() {
                warnings.push(format!("{} no longer recognises {} — the count below \
                                       may be wrong", s.name(), self.site));
            }
            s
        }
        // Nothing declared. Ask the registry, and if nothing speaks, fall back —
        // but record a LEAD with its measurements first (§17).
        None => registry::recognise(&seed).unwrap_or_else(|| {
            leads.record(&self.site, Measures::of(&seed));
            registry::SITEMAP
        }),
    };
    let _ = self.spoke.set(strategy);

    let found = strategy.enumerate(&seed)?;    // ← was self.discoverer.discover(…)

    Enumeration {
        resources: found.addresses.map(|a| Resource::new(self.id, absolute(a, &seed))),
        notes:     found.notes,      // the strategy explains itself. No renderer edited.
        figures:   { "declared_total": found.declared_total },
        warnings,
    }
}

// ← was the hardcoded "sitemap". Read after enumerate, which is when discover writes
// the DiscoveryRun, so the run records what actually spoke rather than what was hoped.
fn method(&self) -> &'static str {
    self.spoke.get().or(self.named.as_ref()).map_or("sitemap", |s| s.name())
}
// …so `sources::infer` reads the strategy back out of the store alone — the same way it
// recovers site-from-channel today. No new mechanism: `Source::method` is already
// documented as "the discriminator that later recovers a Source's kind from the store".


// ── collect → SiteSource::acquire ───────────────────────────────────────────
fn acquire(&self, resource, progress) -> Vec<Acquired> {
    let fetched = self.fetch_reporting(&resource.natural_key, false, progress).await?;

    // The branch that matters, and it does not test the strategy's NAME.
    let enclosed = match self.strategy.addresses_are() {
        Addresses::Pages     => self.enclosures(&fetched, base),
        Addresses::Documents => vec![],
    };
    …
}


// ── extract ─────────────────────────────────────────────────────────────────
// NO branch. None. This file cannot name a strategy, because nothing passes one in,
// and that is enforced by the signature rather than by a rule. This is §3.
let kind = ContentKind::classify(&meta, &bytes);   // declared → magic → served name
for reader in extract::readers_for(kind) { … }     // an ordered list. Data, not code.
```

**The cost, counted.** One field on `SiteSource`, one line changed in `enumerate`, one
`match` in `acquire`, one `&'static str` that stops being a literal, and one optional key in
the config block. Everything else — `Note`, `figures`, `method`, `infer`, `readers_for` —
was already the right shape. That is not luck. §6 lists the three earlier times this
codebase made the same call.

---

## 16. One file per strategy

`strategies/` mirrors `sources/`, which is the layout this codebase already proved:
`sources/mod.rs` holds the trait plumbing, `site.rs` and `channel.rs` hold one Source each,
and `mod.rs` re-exports them.

```
crates/centinel-core/src/strategies/
├── mod.rs             BUILT  Recognition, Keyed — the shared vocabulary, and only it
├── crawl/                    WHERE ARE THE ADDRESSES
│   ├── mod.rs         BUILT  the trait, Seed, Walk, the registry
│   ├── sitemap.rs     BUILT  a standard       — ported from the old Discoverer
│   ├── listing.rs     BUILT  a server default — an open directory index
│   ├── index.rs       —      the address set is on the page, not in a link (§10.3)
│   └── none.rs        —      a query box, recognised and REFUSED (§7 rule 3)
└── read/                     WHAT DOES THIS DOCUMENT SAY
    └── mod.rs         BUILT  the trait and the registry. **No strategy is registered.**
```

**`Strategy` was one word doing two jobs, and the split is a correction.** What was built
recognises a *site* and returns *addresses*; nothing in it has an opinion about the bytes at
those addresses. `hillsclerk.com` is the proof: recognised by `sitemap`, 177 addresses
without a mistake, and 23,213 characters of navigation for a page whose content is one
sentence. No crawl strategy could ever have caught that, so the read side is its own
registry rather than a hook on this one.

**`read/` is empty on purpose, and it is wired anyway.** Every extraction fault in
`FIELD-NOTES.md` — the fused table, the spelled-out image, the `data:` URI, the
octet-stream PDF — was a framework defect any site triggers, and each was fixed in the
framework rather than per site. The two outstanding read faults are handled the same way:
navigation-instead-of-article is removed corpus-wide by `boilerplate`, and a `var pdfURL` is
an enclosure question. So nothing has earned a read strategy yet. `extract::derive` consults
the registry on every document regardless, and a test registers one to prove it wins over
the content kind's readers — an empty registry that is wired costs an `is_empty` check, and
an unwired one costs a refactor at the moment somebody finally has a shape to add.

**This is the target, not the directory.** Two of the four crawl strategies exist. `index` is build-order
item 6; `none` is ten lines and still blocked, because *"no collection strategy"* answers a
question nothing asks yet — dropped into a `discover` run it enumerates zero and reads as a
failure. It wants `investigate` first.

**There is no `onbase.rs`, and §15 is not an instruction to write one.** OnBase sits at one
sighting, and §5 is explicit that one sighting is a note rather than a strategy. §15 spends
that example on the *shape* — how recognition, evidence, canonicalisation and a declared
ceiling fit together — precisely because writing it as a file would break the rule the file
above it states. A second city on OnBase changes that; nothing else does.

`sequence` (§10.5) has no row at all, deliberately. Two sightings promoted it to a shape
and not to a build.

**A file is the unit of contribution.** One file is what a reviewer reads, what a pull
request adds, and what `git blame` attributes. A strategy split across a shared match arm
and a helper module is a strategy nobody can review in one sitting.

**The registry collects itself.** `inventory` is already a dependency and `op.rs` already
does exactly this — `inventory::collect!(OpDef)`, with `centinel-macros` submitting one
`OpDef` per op. A strategy registry is the same mechanism a second time:

```rust
// strategies/crawl/mod.rs
inventory::collect!(StrategyDef);

/// Every strategy, in a stable order. The registry holds no `match` and no list literal.
pub fn all() -> Vec<&'static StrategyDef> { … }

mod sitemap; mod listing; mod index; mod none; mod onbase;

// strategies/crawl/onbase.rs
inventory::submit! { StrategyDef { name: "onbase-agenda", make: || Box::new(OnBaseAgenda) } }
```

So adding a strategy is **one new file plus one `mod` line**. The `mod` line is not
overhead: it is the compiler's proof that the file is in the build, and a strategy that
silently failed to register would be indistinguishable from one that recognises nothing.

---

## 17. Leads — what nothing recognised

A **Lead** is a host that no strategy recognised, recorded with the measurements that would
justify writing one. It is recorded and not acted on. The operator promotes it.

**A Lead is not a Crumb, and the difference decides the action.** A Crumb is an address on
*another* host, seen and deliberately not followed. A Lead is *this* host — already
collected, and possibly collected badly. A Crumb asks *should we go there?* A Lead asks
*is what we already have any good?*

### Falling back is not the same as recognising

`sitemap` **recognises itself**: `robots.txt` names a `<sitemapindex>`, and entry 4 system A
is a clean, correct, evidenced recognition. That writes no Lead.

The fallback is a different event. Nothing spoke, and `sitemap` ran anyway because it is the
best available guess. Today those two outcomes are byte-identical in the store — both record
`method = "sitemap"` — and this section exists to separate them.

### What a Lead measures

Five measures, each taken from the entry that proves it works. None needs a strategy.

| Measure | Entry | What a bad value says | Status |
|---|---|---|---|
| share of extracted text that is link text | hillsclerk — **7 pages at 82–85%** | the reader returned the menu, not the page | BUILT |
| `<a href>` count against `<script>` bytes | 1 and 2 | the address set is on the page and not in a link | BUILT |
| sitemap: none declared, none at `/sitemap.xml` | 1 | there is no declared surface to walk | BUILT |
| characters extracted per KB of seed | 2 — 94,125 bytes → **695 chars** | *nothing reliable* | **WITHDRAWN** |
| distinct extracted lengths across N resources | 1 — **75 identical** nav menus | the corpus gained N copies of one page | superseded |
| a declared total that discovery did not reach | 2 and 3 | build item 4 supplies this number | — |

**Characters per KB was withdrawn, and the reason is worth keeping.** Run against the fifty
hillsclerk documents it fires on **42, of which 41 are good pages**, and the sign is
backwards: the seven ruined reads sit at 111–117 characters per KB and the healthy ones at
2.4–2.7. A modern template weighs 200 KB whatever it holds, so the ratio measures the CMS,
not the read. It looked convincing because the two points that set it — OnBase at 7.6 and a
bare IIS listing at ~980 — are different *kinds of document*, not a good read and a bad one.
What it was meant to catch is caught by the script share, on the same OnBase evidence. The
number is still reported and now raises nothing.

The fourth measure is **superseded** rather than withdrawn: repeated text is no longer
counted per address and left for a person to notice, it is removed before indexing by
`crate::boilerplate`.

**~~A Lead needs a bad measure, not only a failed recognition.~~** Half right, and the wrong
half cost a corpus. A Lead does need a bad measure — but this section also gated the
measuring itself on nothing having recognised the seed, and `hillsclerk.com` is recognised
by `sitemap`, enumerates cleanly, and hands back menus. The gate made the tool structurally
unable to report the only thing wrong with it. **Every address is measured now**, recognised
or not: recognition says how to find the pages and says nothing about reading them.

### Why this is the most valuable of the three

§5 says a strategy merges at two sightings. **Nothing currently supplies the second one.**
Four entries came from a person walking four sites by hand over two days, and that does not
reach 67 Florida counties, let alone 3,000.

A Lead list makes the second sighting arrive by itself. Collect twenty `.gov` sites, and the
ones that share a fingerprint sort next to each other with the evidence already attached.
That also turns §12 from a manual grep into a question the store answers — and §12 is the
test that decides whether any of this file is worth building.

```
centinel investigate --leads

  3 hosts collected by fallback, ranked by what looks wrong

  stories.opengov.com          75 resources, 74 identical extractions, no sitemap
  apps.tampagov.net            2,606 declared in the page footer, 53 collected
  tampagov.hylandcloud.com     94 KB in, 695 chars out, 0 anchors, 41 KB of <script>
```

---

## 18. The skill — teaching the method, not the conclusions

The last item, and it must stay last.

```
contrib/
├── centinel.toml.example
└── skills/
    └── write-a-strategy/
        └── SKILL.md
```

**`contrib/` and not `.claude/`, on purpose.** The skill is a contribution guide that
happens to be machine-readable, so it belongs beside the config example a contributor
already reads — versioned with the interface it documents, and reviewed in the same pull
request that changes that interface. `.claude/` is one tool's directory in one checkout; a
skill written for whoever adds the next strategy must not be.

**What it carries is the method, not the findings.** Four site entries and five framework
fixes came out of one repeatable walk. The walk is the asset. `FIELD-NOTES.md` records what
that walk found; the skill records how to do it again on a site nobody has seen.

### The walk

1. `centinel check <url>`. It fetches, runs the **real** extractor, follows enclosures, and
   stores nothing. Read the bytes-in against the characters-out.
2. Compare them. 94,125 bytes producing 695 characters means the content is not in the text.
3. Read the raw HTML. Find where the address set lives: an attribute, a literal inside a
   `<script>`, an ordinary link, or a directory index.
4. Fetch one address by hand, then vary it. Watch for decoys — `DownloadFile` and
   `DownloadFileBytes` are one word apart and only one of them serves a document.
5. Write the `FIELD-NOTES.md` entry **first**, judged against the three levers.
6. Only at **two sightings**, write the strategy.

Steps 3 and 4 are a person reading source code to find an address. That is the honest cost,
it is not automatable, and the skill must say so rather than imply a crawler can do it.

### What the skill must refuse

Each of these is a rule an agent will otherwise break, and the entry that proves the cost:

- **Never key on a jurisdiction.** Key on a product, a framework, a server default, or a
  standard (§5).
- **An extraction fault is a framework fix and never enters a strategy** (§3). This is the
  one a helpful agent breaks first, because working around a fused table locally looks like
  finishing the task.
- **`None` is a valid answer**, and *"no collection strategy"* is a valid finding (§7.3).
- **A parse, never a scan** (§10). Entry 2 fault 1 is what a scan produces.
- **Recognition carries evidence, not a verdict** (§7.1). Entry 1 is what a confident
  wrong answer costs: 75 successful acquisitions and not one budget figure.
- **Two sightings, and name both hosts in the pull request.** A reviewer asks one question,
  and the answer is checkable.

### What it produces

A `FIELD-NOTES.md` entry, a `strategies/crawl/<name>.rs`, and a test. The test's fixture is a blob
sha that is **already in the store** — §9 point 3 — because a strategy is a pure function
over bytes and the bytes were kept.
