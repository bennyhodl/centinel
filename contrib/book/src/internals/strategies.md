# Strategies

A **strategy** answers one question: *where are the addresses?*

It is a sitemap index, a directory listing, a vendor product that serves its records a
particular way. Recognition and enumeration are the same object, and that pairing is the
design rather than a convenience.

```rust
pub trait Strategy: Send + Sync {
    /// The name recorded on the DiscoveryRun.
    fn name(&self) -> &'static str;

    /// What did you see, and how sure are you? `None` is a valid, common answer.
    fn recognise(&self, seed: &Fetched) -> Option<Recognition>;

    /// Produce the complete address set.
    fn enumerate(&self, seed: &Fetched) -> anyhow::Result<Enumerated>;

    /// Did this strategy name **pages**, or did it name **documents**?
    fn addresses_are(&self) -> Addresses { Addresses::Pages }
}
```

- You cannot add a strategy without teaching it to recognise itself.
- You cannot recognise a shape you cannot then handle.

Without that invariant the registry rots into a set of confident half-answers, and a
confident half-answer is the most expensive thing in this system.

## The unit of contribution is a strategy, never a site

A strategy keys on a **product**, a **framework**, a **server default**, or a
**standard**. Never on a jurisdiction.

```rust
pub enum Keyed {
    Product(&'static str),        // "Hyland OnBase Agenda Online"
    Framework(&'static str),      // "ASP.NET WebForms + Telerik RadGrid"
    ServerDefault(&'static str),  // "IIS directory index"
    Standard(&'static str),       // "sitemap.xml"
}
```

Every one of those ships to many cities, which is what makes the work amortise.
Recognising Hyland OnBase collects every city running OnBase; teaching it Agartha collects
Agartha. `Keyed` has **no `Jurisdiction` variant**, so the rule is enforced by the type
rather than by review.

A strategy that could key on a city is a fork with extra steps.

### Two sightings, not one

A strategy merges when it recognises **two** deployments. A reviewer can ask a pull
request one question — *which two hosts does this recognise?* — and the answer is
checkable. OnBase at one city is a note. OnBase at two cities is a strategy.

## More specific wins

```rust
pub fn specificity(self) -> u8 {
    match self {
        Self::Product(_) => 0,
        Self::Framework(_) => 1,
        Self::ServerDefault(_) => 2,
        Self::Standard(_) => 3,
    }
}
```

Lower is more specific, and this is not a tiebreak detail. It is the difference between
collecting a site and collecting its front door.

A real host running OnBase also serves a `robots.txt`, so both the product strategy and
the sitemap standard answer for it. **The sitemap answer is true** — there is a sitemap and
it enumerates cleanly. It is also nearly worthless, because the meetings are in a JSON
literal that no sitemap names.

A recogniser that keyed on the vendor saw more than one that keyed on a standard every
server can satisfy, so it ranks ahead of it.

## Pages or documents

`addresses_are` is the third method and it earns its place.

`sitemap` names **pages**, so a page can still hide a PDF inside a viewer and the
[enclosure](extract.md#enclosures) scan must run. A product strategy that names
**documents** leaves nothing to find, and running the scan there is exactly what once
invented URLs like `/251agendaonline/.pdf?documentType=`.

Without this method, `acquire` would have to test the strategy's *name* — the `match` the
registry exists to prevent.

## The registry holds no `match`

The registry is a list. Each element tests itself. Nothing dispatches on a kind.

This is content-kind detection one level up: magic bytes are a list of *(signature →
kind)* where each element answers for itself, and a strategy registry is magic bytes for
websites. Registration is link-time, so there is nowhere to forget to add one.

Two strategies are registered today — `sitemap` and `listing` — with `listing` covering
both IIS and Apache/nginx directory indexes.

## Recognition must carry evidence

A wrong recognition is silent, and it produces a run that looks perfect:

> 75 Resources, 75 successful acquisitions, 75 Observations, liveness all `live`. Each
> address is its own placement, so all 75 index. The corpus gains 75 copies of a
> navigation menu that says "Preview link expired", and not one budget figure.

So a `Recognition` carries what was seen, not only a verdict. Five rules follow, and each
one comes from a real failure rather than from caution.

**1. Evidence, not a verdict.** The operator accepts or rejects on what was seen.

**2. `None` is a first-class answer.** A registry that always answers is lying.

**3. "No collection strategy" is a valid, final finding.** A query box behind a
server-side CAPTCHA has no Resource set behind it, and generating case numbers would
*invent* a corpus rather than observe one. The registry must be able to recognise a query
box and refuse it — a strategy that enumerates nothing on purpose.

**4. A recogniser may answer "wrong address".** The visible URL is not always the
fetchable one. A third valid output is a **crumb**: the system you want is on another
host, recorded and not followed. One Source per exact host; the operator promotes crumbs,
and that is what bounds the recursion.

**5. What was recognised is written down, and later disagreement warns.** Sites change.
When a vendor ships a new version and its fingerprint stops matching, the operator gets a
warning — not a silent switch to a weaker strategy and an empty corpus. Same shape as the
pipeline version on an `Underivable`: a verdict belongs to one version and says nothing
about the next.

A `Recognition` also carries **warnings**, which are carried forward into every run rather
than printed once. A host that answers HTTP 200 on its error page is a fact about every
future acquisition, not about the moment somebody noticed.

## What the operator types

`investigate` is not a separate feature. It is the registry's front door — one module, two
callers:

| Caller | Asks the registry |
|---|---|
| `centinel investigate <url>` | *who recognises this?* — prints the answer and the evidence, then stops |
| `centinel run` | *you — do the work* |

```console
$ centinel investigate https://www.valhallaclerk.com/

  seed        https://www.valhallaclerk.com/  →  200, 212 KB, html
  recognised  sitemap (standard)
              robots.txt allows everything and names a <sitemapindex>
              182 child sitemaps
  crumbs      hover.valhallaclerk.com          8 links
              publicrec.valhallaclerk.com      2 links

  centinel source add valhallaclerk --site https://www.valhallaclerk.com/
```

Three outputs: a strategy with its evidence, a set of crumbs, or nothing said plainly.
Promotion needs no new command — `source add` already does it.

A probe is deliberately small: 25 requests, 500 addresses kept. This is a question asked
while deciding whether a host is worth collecting, often about ten hosts in a row, and a
walk that takes minutes is one nobody runs twice. A probe that fills its ceiling reports
`truncated`, so a floor is never printed as a total.

## Recognition is not reading

The registry was briefly a second answer to *how do we get text out of this*, and that was
a mistake worth recording. `valhallaclerk.com` is recognised by `sitemap`, enumerates 177
addresses without a mistake, and hands back 23,213 characters of navigation for a page
whose content is one sentence. True — and it does not follow that the fix belongs beside a
crawl strategy.

Reading belongs to [extraction](extract.md), which already dispatches to an ordered list
of readers. A second registry in front of that list opted out of every invariant the list
exists to hold.

Next: [Acquisition](acquire.md).
