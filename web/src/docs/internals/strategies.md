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
    fn recognise(&self, seed: &Seed) -> Option<Recognition>;

    /// Produce the complete address set, fetching only through `crawl`.
    fn enumerate(&self, seed: &Seed, crawl: &dyn Walk) -> BoxFuture<anyhow::Result<Enumerated>>;

    /// Did this strategy name **pages**, or did it name **documents**?
    fn addresses_are(&self) -> Addresses { Addresses::Pages }

    /// Is the first address the seed itself? Lets the store give back a seed's path.
    fn first_address_is_the_seed(&self) -> bool { false }
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

Fifteen strategies are registered today:

| Strategy | Keys on | Addresses |
|---|---|---|
| `onbase-agenda` | Hyland OnBase Agenda Online (product) | the agenda, minutes and summary PDF of every meeting the application's search lists |
| `legistar` | Legistar, on `{client}.legistar.com` or its Web API (product) | per meeting, the meeting's items from the API, and from the client's InSite site the agenda, minutes and other meeting documents and every attachment of every item's legislation |
| `granicus` | a Granicus `ViewPublisher.php` page or agenda feed (product) | the `AgendaViewer.php` and `MinutesViewer.php` documents it lists, every recorded meeting's caption track (`/videos/{clip}/captions.vtt`), and each item's documents where the agenda was built in Granicus |
| `agenda-center` | a CivicPlus `/AgendaCenter` page or its search (product) | the agenda, packet and minutes of every meeting the module lists, year by year, and each item's documents where the agenda was built in the module |
| `civicclerk` | a CivicClerk portal, `{tenant}.portal.civicclerk.com`, or its API's events address (product) | from the tenant's public API, every event's published files (agenda, packet, minutes, notices) and every agenda item's attachments |
| `civicweb` | an iCompass CivicWeb portal, `{client}.civicweb.net`, or its calendar address (product) | every published meeting's agenda and minutes PDFs, and every document its agenda links for an item, at `/document/{id}/` |
| `openbook` | a Questica OpenBook front door, or the API address of one of its books (product) | each budget book as the one PDF the product publishes of it |
| `opengov-stories` | an OpenGov Stories book's table of contents (product) | every published page the contents name, by its `data-public-url` |
| `campus-suite` | a page assembled by `Campussuite.Widgets` (product) | the site's pages, and every document they link under `/files/` on the site's host, newest upload first |
| `wmis` | a query on the Southwest Florida Water Management District's ERP map layer, whose features link WMIS (product) | every document on file for each environmental resource permit application the query names, newest application first |
| `voterfocus` | a county's VoterFocus Campaign Finance listing, `candidate_pr.php?c=<county>` (product) | every filer's page in every reporting group, and for each filer that reported, every campaign treasurer's report page and the CSV of all its transactions; the filed PDFs are taken from those pages as enclosures |
| `sitemap` | a `Sitemap:` line in `robots.txt` (standard) | every `<loc>` the sitemaps list |
| `listing` | an IIS or Apache/nginx directory index (server default) | the files under the seed's directory |
| `feed` | an RSS, Atom or iCalendar seed (standard) | the feed itself; its entries are split at [acquisition](acquire.md#feeds) |
| `links` | nothing — it never recognises | the seed, and the readable pages it links to inside its scope |

The six products publish council agendas and minutes for most of Tampa Bay, and each
reads its meetings from the product's own search or data rather than by following links.
The seed bounds a walk, as a path bounds `links`: an OnBase search address with a date range
(`…/Meetings/Search?dropid=11&dropsv=08/22/2026&dropev=11/20/2026`), a Legistar or
CivicClerk API events address with an OData `$filter`, a CivicWeb calendar address
(`…/Services/MeetingsService.svc/meetings?from=2026-08-22&to=2026-11-20`), or an Agenda Center search
(`…/AgendaCenter/Search/?term=&CIDs=all&startDate=08/22/2026&endDate=11/20/2026`) covers
that range, and the application's front door covers everything. Agenda Center lists one
year per category on a page, so its walk asks one search per year. OnBase's search answers at most a hundred meetings and says so, so a
capped date range is halved until every answer is whole.

`civicweb` reads the JSON service the portal's calendar script calls: one request lists
every meeting in a range with whether it is published, and one more per published meeting,
newest first, names its agenda and minutes and carries the agenda's HTML, whose links are
each item's documents. Only a document's id decides it, so each is kept as
`/document/{id}/` whatever name and `handle` the agenda wrote. Every portal's robots.txt
turns away all but six search engines, so a source needs the operator's ruling
(`--ignore-robots`) before it collects anything.

`wmis` is bounded the same way, by its seed: a query on the District's ERP map layer, whose
`where` (a date, fixed or `CURRENT_DATE - 30`) and `geometry` (a county's outline) are kept,
while the walk sets the order, newest application first, and the paging. The layer names
each application's WMIS page, and the page lists every document on file.

Two more products publish budget books. OpenBook draws a book from a JSON API and
publishes it whole as one PDF; the walk reads the front door's cards, or the one book a seed
names by its API address (`…/api/digitalbudgetbookviewer/versionIds/FY26AdoptedBudget`), and
keeps each book's PDF. The book's own link, `#/budget-book/…`, cannot be a seed: the fragment
never reaches the server or the strategy. OpenGov Stories
serves a book's table of contents at an editor's preview address whose entries keep each
page's public address in `data-public-url`, not in `href`; the walk is the seed, and costs no
request.

`campus-suite` is the website product most Hillsborough community development districts
run. It serves no sitemap, and its file lists put 39 to 262 documents on one page, oldest
first — past the 25 the enclosure scan keeps from a page — so it walks the site as `links`
does and keeps every listed document as an address of its own. It does not follow the
calendar, which links the next month forever; the calendar's attachments are copies, on
another host, of files the site lists, and a walk warns about any that are not.

`voterfocus` collects campaign finance rather than meetings: the reports a county Supervisor
of Elections publishes for local candidates and committees. A county's page covers every
reporting group, and `…&el=<group>` covers one. The reports name contributors with their
street addresses, and the recognition says so before anything is collected.

### When `robots.txt` says no

Granicus, CivicWeb, and the hosts Legistar's Web API names its documents on
(`*.legistar1.com`, `legistar2.granicus.com`), serve `User-agent: *` / `Disallow: /` and
admit search engines. The rules are honoured, and a run that collected nothing because of them says so
by name instead of reporting an empty site. `legistar` reads the same documents from the
client's InSite site, `{client}.legistar.com`, which serves no robots.txt. A strategy
whose addresses are on another host than the seed asks that host's rules, not the seed's.

An operator who has read the evidence can rule that a host's robots.txt does not bind a
source: `ignore_robots = true` in its definition, written by `source add --ignore-robots`.
`investigate --ignore-robots` shows what that would collect first. `Crawl-delay` and the
source's request rate still apply, and every run notes that the rules were set aside.

## When nothing can be read

With nothing pinned and nothing recognised, `sitemap` runs anyway and tries
`/sitemap.xml`. A walk that then reads **nothing** — every sitemap refused or would not
parse — gives way to `links`: the seed, and the pages it links to, followed breadth-first
inside the seed's **scope** under the same request budget, pacing, `robots.txt` and
cancellation. A site with no sitemap is not a site with no pages; a Google Site is the
common case.

Two outcomes that used to look identical are now told apart. A sitemap that was read and
lists nothing is an empty set. A sitemap that could not be read is failed discovery, and
the run says so with a `discovery` note and the `discovery_failed` figure. When even the
seed refuses, the seed is still the one address, so acquiring it records it `blocked`
rather than the run recording nothing.

A **pinned** strategy does not give way. It is what the operator accepted, and it keeps
that behaviour; its failure is reported, not replaced.

### Scope

A seed's path is part of what the operator said. `linktr.ee/TampaDSA` is one account on a
platform, and `actionnetwork.org/forms/join-…` is one form. `links` stays on the seed's
origin and under the directory its path names — `/TampaDSA` covers `/TampaDSA` and
`/TampaDSA/…`, and a seed at `/` covers the host. Nothing names a platform: the rule is
the same everywhere. A host's own sitemap is not narrowed by it, because a sitemap is the
publisher's statement of what the site holds.

## Feeds a page publishes

Whatever walked the site, the seed page is asked which feeds it declares: a
`<link rel="alternate">` to RSS or Atom, a link to an `.ics` or `webcal:` address, or a
Google Calendar embed, whose public `.ics` address is read from the embed's `src`. One
inside the seed's scope is an address like any other. One elsewhere is a note naming the
address to add as its own source, and is not followed.

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
