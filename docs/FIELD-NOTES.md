# Field notes

What real government sites do, and what it would cost to collect them.

This file is a **catalogue, not a plan**. Each entry records one site: what a person sees,
what Centinel makes of it, where that goes wrong, and what would have to be built. No entry
commits the project to building anything. The point is to see enough sites that the shared
shapes become obvious, and to let the strategy fall out of the evidence rather than out of
the first site that resisted.

**Read [The one every entry shows](#the-one-every-entry-shows) first.** It is the only
conclusion the whole file agrees on, and it did not come from any single entry.

Vocabulary is `CONTEXT.md`'s. In particular the three levers an entry is judged against:

| Lever | The question |
|---|---|
| **Content kind** | did classification name the right thing? |
| **Reader list** | did the readers for that kind produce the text? |
| **Enclosure** | is the real document one address away? |

A site that fails none of these but still yields nothing has failed somewhere else —
usually **enumerate**, and that is worth saying out loud, because a discovery fault and an
extraction fault look identical from a search box: no results.

**Rule for entries: no site rules.** A finding earns a build only when it generalises past
the site that produced it. "OpenGov needs X" is a note. "Sites with no sitemap need X" is a
lever.

---

## Index

| # | Site | What breaks | Stage at fault | Generalises to |
|---|---|---|---|---|
| 1 | OpenGov Stories (`stories.opengov.com`) | 75 addresses, 1 document | enumerate | any site with no sitemap whose index page holds the address set |
| 2 | Hyland OnBase Agenda Online (`*.hylandcloud.com`) | search caps at 100 in silence; error pages answer 200; the HTML view is 84% base64 while a clean PDF sits at a derivable address | enumerate, collect **and** extract | every city running OnBase; every ASP.NET site that answers 200 on error; every site at all, for the base64 |
| 3 | CTTV captions (`apps.tampagov.net/cttv_cc_webapp`) | table cells fuse into one run-on line; 2,606 transcripts behind a `__doPostBack` pager | extract **and** enumerate | **every HTML page with a table**; every ASP.NET WebForms grid |
| 4 | Hillsborough Clerk (`hillsclerk.com`) | ~6 GB of open bulk files nobody looked for; every `.csv` unextractable because IIS says `octet-stream` | collect | every server that defaults to `octet-stream`; every open directory listing |
| 5 | Six ordinary city and county sites | five of six enumerate perfectly and still lose most of their content; one loses all 579 addresses to a relative `Sitemap:` line | **extract and report** — not enumerate | every CMS site, which is most of them |
| 6 | A full run on the box: 37 Tampa Bay sources | one 830 MB file killed every run; CMS redirect pages are fetched off-host and counted as the city's failures; `<base href>` is ignored, so largo's links resolve to 404s; a footer PDF is fetched once per page (21.5 GB of repeats); a 10 s crawl delay over a WooCommerce shop holds the run ~13 h; extract peaks at ~670 MB per vector PDF and glibc keeps it; one 17,162-font PDF stalls extract with no deadline; index's chrome learner holds every CSV row and is killed | **collect** (memory, redirects), **enumerate** (links), **extract** and **index** (memory, time) | every run on a small machine; every CivicPlus/Granicus "external link" page; every site that declares `<base href>` |
| 7 | Council agendas: OnBase, Legistar, Granicus | robots.txt turned the walk away and the report read "empty"; a strategy's off-host addresses were checked against the seed's rules | **enumerate** and **report** | every vendor that admits only search engines; every strategy whose documents live on another host |
| 8 | Agenda item attachments: OnBase, Legistar, Granicus | the agreements a council votes on are attached to items, and no source collected one: OnBase never opened an item, Legistar's are on robots-disallowed hosts | **enumerate** | every agenda product: the meeting's PDF names the item, the item holds the document |
| 9 | CivicPlus Agenda Center (`/AgendaCenter`) | eight box sources reach it and hold only the front door; seven of the eight modules are empty, and the one that is not lists one year per category, with older years behind a POST | **enumerate** | every CivicEngage site: the module is at the same path, and its search is a GET per year |
| 16 | Procurement: OpenGov, Bonfire, DemandStar, VendorLink, PlanetBids, IonWave, OnBase Public Access, and the governments' own pages | 26 governments and clerks; every vendor platform gates documents behind an account, a bot challenge or `Disallow: /`; the one public system addresses each document by a token that changes per search, and serves scans | **before acquire** — login, challenge, robots, POST; then **identity** and OCR | every procurement vendor; every search whose hits carry no stable address |
| 17 | Public-records request portals: GovQA, JustFOIA, NextRequest | 25 Tampa Bay agencies run a request portal; GovQA's robots.txt turns away every crawler at 11 of 12 hosts, JustFOIA's request list answers a Cloudflare challenge at every host tried, and the two NextRequest portals publish 12 requests and no documents | **enumerate**, closed by the vendor | every GovQA and JustFOIA client; NextRequest only where the agency turns publishing on |
| 15 | Accela Citizen Access (`aca-prod.accela.com/{AGENCY}`): Tampa, Hillsborough, Pinellas | records are a GET and an ArcGIS layer lists their addresses; every attached document is a WebForms postback inside a session that names the record, and has no GET address at all | **acquire** — and the fetch layer, which cannot POST or hold a cookie | every agency on Accela Citizen Access; every WebForms page whose files are `__doPostBack` links |
| 10 | CivicClerk (`{tenant}.portal.civicclerk.com`) | the portal is a script with no meeting in it; the meetings are in a public OData API, fifteen to a page, and the JSON's own document links are blob URLs signed for a day | **enumerate** | every CivicClerk tenant: the API is at `{tenant}.api.civicclerk.com/v1`, and the portal's own download routes are stable addresses |
| 14 | Meeting video: Granicus players and the cities' YouTube channels | the recording is refused (CloudFront 403) but its captions sit beside it at `/videos/{clip}/captions.vtt` — 6,400 hours already transcribed — and a `.vtt` classified `other` and was read by nothing; the box has no channel source and no transcription server | **content kind** and **reader list** | every publisher whose player loads a WebVTT track |
| 19 | Budgets, ACFRs and codes of ordinances: Tampa Bay's nine governments; Questica OpenBook, OpenGov Stories, Municode | adopted budgets on two vendor platforms no crawl reads; ACFRs on finance pages past the 25-document enclosure cap; every code publisher answers a Cloudflare challenge though robots.txt allows | **enumerate**, and **collect** (the enclosure cap) | every OpenBook and OpenGov Stories client; every finance page that lists its years; every code on Municode, American Legal or eCode360 |
| 13 | Special districts and authorities: CDDs, CRAs, Port, airports, transit, water, expressway, MPO, regional council | none is a source but two; 14 of 24 sampled CDDs run Campus Suite, whose file lists put 39–262 documents on a page, past the enclosure scan's 25; 10 of those 14 fail TLS on a new Let's Encrypt root; HART fails TLS on CBC-only ciphers; two products and SharePoint disallow the documents in robots.txt | **acquire** (TLS), **enumerate** (listings), and **robots** | every Campus Suite site; every host on Let's Encrypt's Gen Y chain without the cross-sign; every WordPress page listing more than 25 documents |
| 18 | Southwest Florida WMD permits (WMIS) and governing board | the permit search is a postback and every document opens at a new random address per request; the main site, board packets included, answers a Cloudflare challenge | **enumerate** | every district whose GIS layer names each permit's record page (SFWMD too); every site that serves a file through a one-time redirect |
| 20 | Money and influence: lobbyists, campaign finance, financial disclosures | four counties' campaign finance sit on one vendor nobody collected, and every CSV it exports would have been Underivable (CSV served as `vnd.ms-excel`); Tampa's lobbyist log is a RadGrid behind a POST; Florida's disclosure system is a JSON API; no CivicPlus box source holds one Document Center file | **enumerate**, **content kind**, **enclosure** | every county on VoterFocus; every server that declares CSV as Excel; every CivicEngage site |

**Outcome of the first session:** entries 1–4 produced one agreed feature — **crumbs**. See
[The one every entry shows](#the-one-every-entry-shows). Every other finding stays a note.

**Outcome of the second:** entry 5 inverts the file. Entries 1–4 are exotic systems and all
four fail at `enumerate`; six ordinary sites enumerate cleanly and fail *after*, at the
boundary between what the pipeline measures and what it reports. Five framework fixes fall
out, all of them defects any site triggers and none of them a strategy.

---

## 1. OpenGov Stories — Tampa FY2027 budget book

*Seen 2026-08-07. Entry URL: `https://stories.opengov.com/tampa/79cac2e9-…/published/mHPI_6Pa9?currentPageId=TOC`*

### What a person sees

A published budget book. A left-hand table of contents — Overview, The Recommended
Budget, Forecasts, Department Budgets, Capital & Debt, City Financial Policies, Appendix.
Click a heading and the body changes. The address bar changes with it: only the
`?currentPageId=` value moves.

### What Centinel makes of it

The address bar lies. The server ignores `currentPageId` and returns the same bytes every
time. Page selection happens in the browser.

| Address | Bytes | md5 | Extracted |
|---|---|---|---|
| `…/published/mHPI_6Pa9?currentPageId=TOC` | 214,139 | `5807b755` | 12,989 chars |
| `…/published/mHPI_6Pa9?currentPageId=69f0…7659` | 214,139 | `5807b755` | 12,989 chars |
| `…/published/mHPI_6Pa9` (no query at all) | 214,139 | `5807b755` | — |

So 75 table-of-contents entries are 75 Resources holding one document. The extracted text
is the navigation shell. It also contains the line **"Preview link expired"** — this URL
form is an editor preview, and its token is already dead.

Extraction did its job. There was one page, and it produced that page.

### The real pages exist, and are plain HTML

The public form is `/tampa/published/<slug>`. It is server-rendered. No JavaScript needed.

| Slug | Title | Bytes | Extracted |
|---|---|---|---|
| `0fcj5SYGof` | Financial Forecast | 40,746 | 11,847 chars |
| `-afvrw_1ec` | Fund Descriptions | 64,829 | — |
| `0V4dWv5_mm` | Fire | 32,790 | — |
| `-CiCMYu9gj` | City Council | 24,142 | — |
| `0-2Wz1W5G9N` | Frequently Asked Questions | 28,687 | — |
| `04_H-06AWc` | Enterprise Funds Major Revenues | 28,588 | — |

Distinct bytes, correct titles, clean markdown. Readability plus htmd kept the headings and
the lists, and the title became the `# H1`.

### The issue

**The complete address set is present on the shell page, and it is not in any `href`.**

```html
<a href="?currentPageId=69f0cb1b57e34961bd109fd5"
   class="toc-story-link js-toc-story-link"
   data-id="69f0cb1b57e34961bd109fd5"
   data-public-url="https://stories.opengov.com/tampa/published/g18-buk8eN">
   Online Budget User's Guide
</a>
```

The `href` is a client-side route. The address is in `data-public-url`. There are 75 of
each, one to one.

Nothing else can supply the set:

- `robots.txt` — 404
- `sitemap.xml` — 404
- `/tampa` — 404
- a public page carries **one** sibling link, so a link walk stops on the first hop
- `/tampa/published/mHPI_6Pa9` — 404. The book's own contents page has **no public form**;
  only the UUID preview URL answers.

The shell is the only place the address set exists.

### Against the three levers

| Lever | Verdict |
|---|---|
| Content kind | `html` — correct |
| Reader list | Readability → whole page — correct, and the output is good |
| Enclosure | nothing to follow. No PDF, XLSX, DOCX or CSV link on any page. |

None of them is at fault. `SiteSource` enumerates by walking a sitemap, and there is no
sitemap. **This is an enumerate fault.**

Worth stating plainly, because the failure is silent in the worst way: added as-is, the
source looks healthy. 75 Resources, 75 successful acquisitions, 75 Observations, liveness
all `live`. Each address is its own Placement, so all 75 index. The corpus gains 75 copies
of a navigation menu that says "Preview link expired", and not one budget figure. It is the
`tampa.gov` print-notice failure in a new shape — a page that enters the corpus looking
collected and carrying nothing.

### The way around

Read the address set off the index page instead of off a sitemap. One fetch, one parse, 75
addresses, and each one then collected as ordinary HTML by machinery that already works.

It stays inside what `enumerate` promises: a **complete, bounded snapshot**, taken in one
pass. It is not a recursive crawl, and it does not need a headless browser.

### What would be built

A second enumeration strategy for `SiteSource`: **enumerate from an index page.** A seed
URL, plus where on that page the addresses live.

How general is it: **high.** A `.gov` microsite with a landing page and no sitemap is
common. The current design can only say "walk the sitemap", so every such site is
uncollectable for a reason that has nothing to do with its content.

The one complication this site adds: the addresses are in `data-public-url`, not in `href`.
So the strategy has to name *which attribute carries the address*. That is one config field
— and it is the field that keeps this from being an OpenGov special case, because the next
site will hide its addresses somewhere else again.

Deliberately **not** built: an OpenGov Stories adapter. One site does not earn an adapter.
`CONTEXT.md` is explicit that one adapter is a hypothetical seam.

### Left on the table

**The dollar figures are not in the text.** Each page embeds its charts as iframes to
`tampa.opengov.com/transparency/#/186460/…` — a different host, the address behind a `#`
fragment, drawn by JavaScript. The enclosure rule (one level, same host) correctly excludes
them.

So the narrative is collected and the numbers are not. "The forecast uses projections of
major revenue and expenditure drivers" gets indexed; the General Fund total does not. That
is a separate source, not an extraction fix.

**The seed may rot.** The only address that holds the table of contents is a preview URL
that already reports its own expiry. Before this is worth building, somebody has to find
whether OpenGov publishes a stable per-city listing of published stories. If it does not,
the entry point has to be re-supplied by hand each budget year — which is a fact about the
source, not a bug in the strategy.

---

## 2. Hyland OnBase Agenda Online — Tampa city council agendas and minutes

*Seen 2026-08-07. Entry URL: `https://tampagov.hylandcloud.com/251agendaonline/`*

This is a **commercial off-the-shelf product**, sold by Hyland Software to many cities. The
`251` is Tampa's tenant number. Everything below is a finding about OnBase, not about
Tampa, which is what makes it worth the length.

### What a person sees

A home page listing recent and upcoming meetings, with filters for Meeting Name, Meeting
Type and Meeting Date. An "All Meetings" page with a keyword box and a date-range dropdown.
Click a meeting and an agenda or minutes document opens in a reading pane with a clickable
outline.

### What Centinel makes of it

| Address | Bytes | Extracted | Note |
|---|---|---|---|
| `/251agendaonline/` | 94,125 | **695 chars** | the meeting list is absent |
| `/251agendaonline/Meetings` | 11,053 | 797 chars | readability found 102 chars, fell back to whole page |
| `/251agendaonline/.pdf?documentType=` | 3,860 | 733 chars | **an address that does not exist**, followed as an enclosure |

Three separate faults, in three different stages. Taken one at a time.

### Fault 1 — enclosure detection read a line of JavaScript as an address

`check` followed an enclosure to `https://tampagov.hylandcloud.com/251agendaonline/.pdf?documentType=`.
Nothing on the page links there. The string came out of a `<script>` body:

```js
let link = $("<a>").attr("href",
    "/251agendaonline/Documents/DownloadFile/"
    + encodeURIComponent(doc.UrlFriendlyName)
    + ".pdf?documentType=" + doc.MeetingDocumentType
    + "&meetingId=" + meeting.ID + ...);
```

The scanner matched `.pdf?documentType=` inside a **string-concatenation expression** and
resolved it against the page's base. The result is a plausible-looking URL that names no
document.

**The lever:** enclosure candidates must come from parsed HTML attributes, never from a
scan of the page's text. A `<script>` body is source code, and the URLs in it are templates
with their variables still unsubstituted. This generalises to every page whose JavaScript
builds links, which is most of them.

### Fault 2 — the error page answers HTTP 200

This one is worse, and it is what makes fault 1 expensive rather than merely untidy.

| Requested | Final URL | Status |
|---|---|---|
| `/251agendaonline/.pdf?documentType=` | `/251agendaonline/Error/NotFound?aspxerrorpath=…` | **200** |
| `…/ViewAgenda?meetingId=2500&doctype=` (empty) | `/251agendaonline/Error/InternalServer?aspxerrorpath=…` | **200** |

A missing document and a server crash both come back `200 text/html` with a body reading
*"The web page that you have requested is not available"*.

So `Gone` can never fire on this host. Every dead address becomes a successful acquisition:
an Observation, real bytes, liveness `live`, and a document titled **"Error - OnBase Agenda
Online"** in the search index. Fault 1 invents such an address on every single page, so the
corpus would gain one error document per page collected.

This is the exact inverse of **Blocked**. `Blocked` exists so a live page is not recorded as
deleted; here a deleted page is recorded as live, and its error text is indexed as content.

**The lever:** a source needs a way to say *this final URL is a refusal*. The `Refusal` and
`Liveness` model already has the right words — it just never gets asked, because HTTP said
200 and nothing after that point re-opens the question. Redirection to a path like
`/Error/…` is the evidence, and it is in `final_url`, which `check` already reports.
Generalises to ASP.NET, ColdFusion and most CMS platforms, which answer 200 on error far
more often than they should.

### Fault 3 — the address set is embedded JSON, and there are no links

The home page's meeting list is not markup. It is a JSON literal passed to a function:

```js
showSearchResults(new SearchResults({"MeetingTypes":[…],"DateRangeOptionID":4,
  "Meetings":[{"ID":2933,"Name":"City Council Reguar - October 1, 2026",
   "MeetingTypeName":"Council Regular","IsAgendaAvailable":false,
   "IsMinutesAvailable":false,"Time":"2026-10-01T09:00:00-04:00", …}, …]}))
```

61 meetings, each with an ID, a name, a type, a time, and per-document availability flags.
Not one `<a href>` to a meeting anywhere on the page. Readability discards the `<script>`,
correctly, and 94,125 bytes become 695 characters — a welcome message and a disclaimer.

Same shape as OpenGov Stories, one layer deeper: **the address set is on the page, and not
in a link.** There it was an attribute; here it is a script literal.

### Where the content actually is

Three address forms, and only the third holds text.

| Form | Result |
|---|---|
| `/Meetings/ViewMeeting?id=2500` | a 15 KB shell. No document, no `DownloadFile` links. |
| `/Documents/DownloadFile/<UniqueName>.pdf?documentType=…` | 1,448 bytes of HTML, not a PDF. Needs a `publishId` the search result does not carry. **A decoy** — see below. |
| `/Documents/DownloadFileBytes/<anything>.pdf?documentType=N&meetingId=ID` | **the PDF**, and the best address on this site |
| `/Documents/ViewAgenda?meetingId=2500&type=…&doctype=N` | the same document as HTML |

`doctype` selects it, and `type` is decorative:

| `doctype` | Bytes | What |
|---|---|---|
| `1` | 280,496 | agenda |
| `2` | 314,568 | minutes |
| `3` | 20 | summary — empty |
| *(empty)* | 3,912 | HTTP 200 internal-server error page |

`type=agenda&doctype=2` returns the same 314,568 bytes as `type=minutes&doctype=2`. A
collector that varied `type` would store one document under two addresses and call both
successes.

**And the extraction is excellent.** Minutes for meeting 2500 → **123,172 characters**,
Readability first try, no fallback, headings intact:

```
## CITY OF TAMPA
## Council Regular
## Thursday, December 5, 2024
## 9:00 AM
## PROPOSED REGULAR ACTION MINUTES
```

This is the best text any site in this file has produced. It also does not need OCR, which
the PDF of the same minutes would.

### Fault 4 — 43% of that text is base64

Two `data:image/jpeg;base64,…` URIs, **53,345 of the 123,172 characters**. `htmd` writes the
data URI into the markdown as the image target, so it survives into the derived blob, into
the chunks, and into the embeddings.

```
![A logo of a city Description automatically generated](data:image/jpeg;base64,/9j/4AAQ…
```

Real text: 69,827 chars. The rest is a city logo, spelled out.

**The lever:** strip `data:` URIs from extracted text. This is not a site finding at all —
it applies to every kind and every source, and it is the cheapest fix in this file. It is
never searchable text, it inflates every downstream cost, and it will quietly poison
whichever chunk it lands in.

### Fault 5 — the meeting search caps at 100, and says nothing

Enumeration means POSTing the search form. It needs a session cookie and an ASP.NET
`__RequestVerificationToken` read off the page first, so it is not a GET.

Then the results are capped:

| Date range | Meetings returned |
|---|---|
| 2024-01-01 .. 2024-12-31 | **100** |
| 2024-01-01 .. 2024-06-30 | 77 |
| 2024-07-01 .. 2024-12-31 | 57 |
| 2024-01-01 .. 2024-03-31 | 40 |
| 2020-01-01 .. 2026-12-31 | **100** |

The two halves of 2024 hold 134 meetings. The whole of 2024 reports 100. Seven years also
reports 100.

The response carries **no total, no cursor, and no more-results marker**. A capped answer is
byte-for-byte the same shape as a complete one.

`CONTEXT.md` already states the invariant — *"a truncated snapshot looks exactly like a
source that shrank, so nothing may silently cap one"* — and that is why `run --limit`
applies to collection and not to discovery. The gap is that the rule was written against
*us* capping. Here the **server** caps, and the current design has no way to notice.

**The lever:** a query-based enumerate must treat "exactly the cap" as a suspect answer and
narrow its window until every window comes back short. Bisecting the date range is the
obvious method and needs no knowledge of the product. Without it, a source that quietly
returns the most recent 100 meetings looks healthy forever, and 2015–2023 never enters the
corpus.

### The PDF endpoint, which changes the answer

`DownloadFileBytes` — one word longer than the `DownloadFile` above, and a completely
different thing. It serves `application/pdf`, and it needs **only** `documentType` and
`meetingId`.

Same meeting, same minutes, both addresses:

| Address | Kind | Bytes | Extracted | base64 | **Real text** |
|---|---|---|---|---|---|
| `ViewAgenda?meetingId=2834&doctype=2` | html | 97,487 | 63,463 | 53,353 | 10,110 |
| `DownloadFileBytes/…?documentType=2&meetingId=2834` | pdf | 142,921 | **10,461** | **0** | **10,461** |

The PDF yields *more* real text than the HTML and carries no noise. `pdf-inspector 0.1.7`
read all 5 pages first try — no OCR flag, no fallback to poppler. The markdown is clean:

```
# CITY OF TAMPA
## Council Workshop
## Thursday, June 25, 2026 9:00 AM
# WORKSHOP MINUTES
```

Both renderings come from one Word document — `Creator: Microsoft Office Word`,
`Producer: Aspose.Words for .NET`. The HTML export inlines the letterhead as base64; the PDF
does not. So **the PDF is the artifact to collect here**, and the HTML view is the fallback,
which inverts the note this entry first carried.

**And the filename is decorative.** `…/DownloadFileBytes/anything.pdf?documentType=2&meetingId=2834`
returns the identical 142,921 bytes. Only the query decides. So every agenda and every
minutes PDF is derivable from a meeting ID alone:

```
/251agendaonline/Documents/DownloadFileBytes/<name>.pdf?documentType={1|2}&meetingId=<ID>
```

That collapses collection to: get the IDs from the search JSON, derive two addresses each.
No `publishId`, no per-meeting document listing, no second round trip.

**The hazard it creates.** A decorative path segment means one document has unbounded
addresses. `CONTEXT.md` says a Resource is an address and that four honest rows beat one
confident wrong one — but that rule was written for *a meeting reachable four ways*, where
each path is real. Here the paths are free text. If an enumerator writes the filename from
the JSON's `MinutesUniqueName` and the clerk renames *Proposed* to *Approved* minutes, the
same document arrives at a new address as a new Resource with no history. So the address
this source records has to be **canonical** — ID and document type, and a fixed or dropped
filename.

Which raises the tension in the next fault: the decorative filename is the only part of the
address that carries the meeting's name.

### Fault 6 — the title and the body are at different addresses

The address a person copies from the browser is `Meetings/ViewMeeting?id=2815&doctype=1`.
Fetch it and the document is not there. Fetch the document and the *name* is not there.

| Address | Bytes | Extracted | `<title>` | `<h1>` |
|---|---|---|---|---|
| `ViewMeeting?id=2815&doctype=1` | 15,652 | **267 chars** | `City Council Regular - July 16, 2026 - 7/16/2026 9:00:00 AM` | none |
| `ViewAgenda?meetingId=2815&doctype=2` | 324,623 | **125,786 chars** | **empty** | `CITY OF TAMPA`, `Council Regular` |

`ViewAgenda` returns an HTML **fragment** — a Word export, opening `<html><head>` with no
doctype and an empty `<title>`. Its `<h1>`s are the letterhead, and every Council Regular
meeting ever held has the same two.

So `CONTEXT.md`'s **Title** rule produces nothing here. There is no title to write in as an
`# H1`, so no meeting name enters any chunk's heading path. The date survives as an `## H2`
in the body (`## Thursday, July 16, 2026`), which is luck rather than design — the Word
template happened to put it there. The string a person would search for, *"City Council
Regular July 16 2026"*, exists at one address and the minutes exist at another, and nothing
joins them.

The PDF is no better. `pdfinfo` reports **no `Title` field at all** — only `Author:
DocMaestro`, `Creator: Microsoft Office Word`. So the meeting's name is in exactly three
places, and none of them is the document:

| Where | Usable? |
|---|---|
| the `ViewMeeting` shell's `<title>` | yes, but that address holds no content |
| the decorative filename in the PDF URL | yes, and the server ignores it |
| the search JSON's `Name` and `Time` fields | **yes, and this is the right one** |

**The lever:** the title rule assumes the name and the body arrive together, because on a
`.gov` CMS page they do. That assumption fails for any content endpoint that returns a
fragment or a raw file.

The fix generalises further than "inherit from the referrer": **when a source enumerates
from structured data, the title comes from that data.** The search JSON already holds
`Name: "City Council Workshop - June 25, 2026"`, `MeetingTypeName`, and `Time`. That is a
better title than anything in the document, it is available before the fetch, and it does
not depend on a decorative URL segment that must not be trusted for identity anyway. It
resolves the tension above: the address stays canonical, and the name comes from the record.

Left unfixed, this costs the heading path of the most valuable documents in the corpus —
every chunk of every set of minutes would sit under `CITY OF TAMPA / Council Regular`, which
is identical for every such meeting ever held.

### Against the three levers

| Lever | Verdict |
|---|---|
| Content kind | `html` — correct throughout |
| Reader list | correct, and on the real documents it is very good. But the derived text carries 43% base64, and the fragment has no title to promote. |
| Enclosure | **wrong** — invents an address from a `<script>` body |

And the stage that decides whether any of it happens is `enumerate`, which needs a POST, a
token, a cookie, a JSON parse, and a cap-aware window walk.

### What would be built

Five levers, none of them a site rule, in the order I would rank them:

| | Lever | Generality |
|---|---|---|
| 1 | strip `data:` URIs from extracted text | **every source, every kind.** Cheapest thing in this file. |
| 2 | enclosure candidates from parsed attributes only, never a text scan | every page with JavaScript |
| 3 | a redirect-to-error-path is a `Refusal`, not an Observation | every platform that answers 200 on error |
| 4 | a cap-aware window walk for query-based enumerate | every paged or capped search backend |
| 5 | enumerate from data embedded in a page | shares a root with entry 1's lever |
| 6 | when a source enumerates from structured data, the title comes from that data | any content endpoint that is a fragment or a raw file |
| 7 | an address with a decorative segment must be canonicalised before it becomes a Resource | any download endpoint whose path is free text |

### How the minutes were actually found

Worth recording, because none of it is a crawl and none of it is something Centinel can do.

1. `/251agendaonline/` — 94,125 bytes, 695 chars of welcome text. Nothing.
2. Read the raw HTML and found `showSearchResults(new SearchResults({…}))`. Meeting IDs.
3. `Meetings/ViewMeeting?id=2815` — a 15 KB shell. Title, no document.
4. Read the shell's inline JavaScript and found `loadAgendaDocument()`, which builds
   `/Documents/ViewAgenda?meetingId=2815&type=…&doctype=N`.
5. Fetched that. 324,623 bytes, 125,786 chars, the whole minutes.

Steps 2 and 4 are **reading source code to find an address**. That is the honest cost of
this site: a person had to read the JavaScript once. What that buys is a rule that then
holds for every meeting and for every city on OnBase — which is the argument for an adapter
whenever a second OnBase city turns up, and the argument against one until then.

Deliberately **not** built: an OnBase adapter. It is a real candidate — unlike OpenGov, this
is one product across many cities, so a second city on OnBase would make the seam real. But
one sighting is one sighting.

### Left on the table

- **Corrected.** This entry first recorded that PDFs were probably out of reach and probably
  not worth chasing, on the evidence of `DownloadFile`. That was wrong on both counts.
  `DownloadFileBytes` serves them, needs nothing but a meeting ID, and produces better text
  than the HTML. Recorded rather than quietly edited, because the mistake is instructive:
  **two endpoints one word apart, one a decoy.** A site walked by hand hides that; the
  browser's own network log would have shown it in a second.
- **No media.** `HasMedia` is false on every meeting sampled, across 2024 and 2026. Council
  video lives somewhere else.
- **Meeting IDs are not sequential and not date-ordered.** The 2024 search returned IDs from
  130 to 2555. Guessing IDs is not an enumeration strategy here.

---

## 3. CTTV closed-captioning archive — 2,606 verbatim council transcripts

*Seen 2026-08-07. Entry URL: `https://apps.tampagov.net/cttv_cc_webapp/`*

The richest source in this file, and the one whose faults cost the most. It is also the
answer to entry 2's *"No media"* note — and the reason that note mattered less than it
looked.

### What a person sees

A table of council meeting transcripts, newest first. Four columns: transcript number, date,
meeting name, and one to three **▶ Watch** buttons linking to YouTube. Fifty rows, then a
numbered pager. The footer states the size of the archive out loud:

> **2606** items in **53** pages

Click a transcript number and you get the whole meeting, spoken word by spoken word.

### What is behind one link

| | |
|---|---|
| Address | `Agenda.aspx?pkey=2689` |
| Served | 919,958 bytes |
| Extracted | **187,924 characters** |
| Structure | 1,417 timestamps, 17 named speakers |

```
[**9:03:54AM >>**](#t90354AM)**ALAN CLENDENIN**:
GOOD MORNING, EVERYBODY.
WELCOME TO TAMPA CITY COUNCIL.
...
[**9:04:29AM >>**](#t90429AM)**LYNN HURTAK**:
```

Timestamped, speaker-attributed, and anchored. Extraction handled it — title promoted,
speakers bolded, timestamps kept as links. This is the single best document any site in this
file has produced.

**And the archive goes back twenty-two years.** `pkey` is a small integer, and low ones hold
real speech:

| `pkey` | Bytes | Date | `>>` speech markers |
|---|---|---|---|
| 1 | 223,224 | — | 57 |
| 100 | 181,659 | — | 167 |
| 500 | 263,913 | — | 275 |
| 1000 | 245,254 | **Thursday, August 26, 2004** | — |
| 2000 | 946,854 | Thursday, February 9, 2017 | 361 |
| 2689 | 919,958 | Thursday, July 16, 2026 | — |

2,606 transcripts at roughly 100,000–190,000 characters each is on the order of **300
million characters of attributed public speech** — larger than everything Centinel currently
holds.

### The finding that changes the pipeline

Centinel has a whole `transcribe` stage: `yt-dlp`, `ffmpeg`, a whisper worker, model
weights, a stall-timeout heartbeat, and hours of compute per meeting. Every row of this
table links the YouTube video that stage exists to process.

**The city already published the transcript, and it is better than the one whisper would
produce.** It carries speaker names, which whisper does not. It carries wall-clock
timestamps. It costs one HTTP fetch instead of hours of inference.

The lever is a *precedence* rule, not a new stage: **where a publisher's own transcript
exists, transcription is redundant work that produces a worse artifact.** That belongs in
the `transcribe` skip predicate's neighbourhood — which today subtracts *blobs derived by
the transcriber from the audio blobs*, and so can only ever ask "have we transcribed this",
never "did somebody already do it better".

Two honest caveats, and neither changes the ranking:

- The header disclaims it: *"AN UNEDITED VERSION OF REALTIME CAPTIONING WHICH SHOULD NEITHER
  BE RELIED UPON FOR COMPLETE ACCURACY NOR USED AS A VERBATIM TRANSCRIPT."* It is CART
  captioning, not a court record. It is still far better *searchable* text than whisper
  output, and the disclaimer is itself in the text, so a reader is told.
- The transcript and the video are two Resources, which is correct and honest under
  `CONTEXT.md`'s rule. The point is not to merge them; it is that the expensive one need not
  be derived when the cheap one exists.

### Fault 1 — table cells fuse in the extracted text

The index markup is clean, semantic HTML:

```html
<td><a href="Agenda.aspx?pkey=2693">Transcript #2693</a></td>
<td width="300">8/3/2026</td>
<td>Tampa City Council Special Discussion</td>
<td><a class="watch-pill" href="https://www.youtube.com/watch?v=MzXMEpSFjp0">▶ Watch</a></td>
```

The extracted markdown is **two lines**. One is the title. The other is 10,736 characters
holding all fifty rows, with no delimiter between any cell:

```
[Transcript #2693](…)8/3/2026Tampa City Council Special Discussion [▶ Watch](…)[Transcript #2692](…)7/30/2026Tampa City …
```

`8/3/2026Tampa City Council Special Discussion`. The date and the meeting name are one
token. `htmd` renders `<td>` inline with no separator and no row break, so a table becomes a
run-on paragraph.

**This is the widest-reaching finding in the file.** Government sites are made of tables —
budget line items, salary schedules, permit registers, election returns, bid tabulations. On
every one of them, right now, the number fuses to the label it belongs to and the row
boundary is gone. Any chunk drawn from such a page mixes dozens of unrelated records, and no
search can separate them.

**The lever:** render `<table>` as a markdown table, or at minimum emit a cell separator and
a row break. It is one extractor change, it needs no per-site knowledge, and it improves
every HTML source already in the store.

### Fault 2 — the pager is a Telerik postback

The address set is not reachable by URL. Pagination is an ASP.NET WebForms grid:

```html
<a title="Go to Page 3" href="javascript:__doPostBack('ctl00$MainContent$RadGrid1$ctl00$ctl02$ctl00$ctl09','')">
```

`__doPostBack` appears 27 times, `__VIEWSTATE` four times, and there is no page parameter to
put in a URL. Page 2 of 53 requires POSTing the view state back. Same family as entry 2's
token POST, one step worse: the state is opaque and must be carried forward from the
previous response.

`Agenda.aspx?pkey=N` *is* a plain GET, so the documents are individually reachable. Only the
**list** of them is locked behind the grid.

### The mitigation this site hands over — and its limit

`pkey` runs 1 to 2,693 and the grid declares **2,606 items**. So the identifiers are dense,
and counting down from the newest would reach nearly everything.

That is **not** enumeration, and `CONTEXT.md` is why: a DiscoveryRun is *a full snapshot of
the Resource set one enumeration observed*, and a walk over guessed integers observes
nothing — it cannot distinguish a gap from a deletion, and entry 2 already showed a sibling
system whose IDs were neither dense nor date-ordered.

What the site does hand over is better than the guess: **it states its own total.** 2,606 is
printed in the footer of every page of the grid.

**The lever:** when a listing declares how many items it has, record that number and check
the snapshot against it. A DiscoveryRun that found 50 while the page said 2,606 is
*provably* truncated, and can refuse instead of silently recording that the source shrank.

This is the exact counterpart to entry 2's fault 5. There, the server capped at 100 and
declared nothing, so truncation was undetectable from the response alone. Here the same
truncation is detectable for free. Two sites, opposite ends of one problem — which is what
makes "check the snapshot against a declared total" worth building rather than guessing at.

### Against the three levers

| Lever | Verdict |
|---|---|
| Content kind | `html` — correct |
| Reader list | **excellent on the transcript, broken on the index.** Tables fuse. |
| Enclosure | correctly quiet. The ▶ Watch links are YouTube — a different host, so not followed. |

68 YouTube links across 50 rows, 53 unique videos. Under the current rule they are out of
scope, and that is right: they are a *different source*, not an enclosure. Worth recording
that this page is where the join between a transcript and its video is written down, if that
join is ever wanted.

### What would be built

| | Lever | Generality |
|---|---|---|
| 1 | render tables with cell and row boundaries | **every HTML source in the store.** Widest reach of anything in this file. |
| 2 | check a DiscoveryRun against a declared total | any listing that prints its own count; pairs with entry 2 fault 5 |
| 3 | prefer a published transcript over transcription | any source that publishes captions beside its media |
| 4 | walk a WebForms grid by carrying view state | ASP.NET WebForms and Telerik RadGrid, which is a large share of municipal sites |

Ranked that way on purpose. Lever 1 is not about this site at all and improves everything
already collected. Lever 4 is the one this site *needs*, and it is the most fragile.

### Left on the table

- **Dates are missing from old transcripts' body text.** `pkey` 1, 100 and 500 have speech
  but no parseable date header; the format changed at some point before 2017. The date is in
  the index row, at the address the pager guards. Same shape as entry 2's fault 6 — **the
  title and the body are at different addresses** — and now at its third sighting.
- **Cost.** 2,606 documents at 180 KB–950 KB is roughly 1–1.5 GB served, and the extracted
  text will be a large multiple of the current corpus. Worth planning rather than discovering.
- **Whether the index is the only listing.** The pager is the only enumeration seen. If a
  date-filtered or printable view exists, it would sidestep lever 4 entirely and is worth
  ten minutes with a browser network log before anything is built.

---

## 4. Hillsborough Clerk of Circuit Court — court records

*Seen 2026-08-07. Entry URL: `https://www.hillsclerk.com/`*

The first entry where the premise turned out to be half wrong, and where the finding is not
a defect but a **path nobody had looked for**.

### The premise, tested

*"You need case numbers or specific queries."* True of the search interface. Not true of the
clerk, who publishes the bulk files openly and has since at least 2014.

This is one hostname hiding three systems, and they need completely different answers:

| System | Host | What it is | Enumerable today? |
|---|---|---|---|
| A — the CMS | `www.hillsclerk.com` | Liferay site, policy and service pages | **yes, today** |
| B — HOVER | `hover.hillsclerk.com` | case search: case no., citation, party name, date range | **no, and correctly so** |
| C — public data | `publicrec.hillsclerk.com` | an open directory of bulk files | **yes, trivially** |

### System A — already collectable

`robots.txt` allows everything (`Disallow:` empty) and names a sitemap. It is a
`<sitemapindex>` with **182 child sitemaps**, and `discovery/sitemap.rs` already handles
index-to-urlset nesting — the module header says so and the parser has the arm. So the
policy side of the clerk needs nothing built. Add it as a source and it works.

Extraction is unremarkable and fine: the *Confidential Information in Court Records* page,
212,480 bytes → 3,088 chars, clean.

### System B — search-only, and the right answer is to leave it

HOVER states its own footing:

> The Florida Supreme Court has authorized the Hillsborough County Clerk of Court and
> Comptroller, 13th Judicial Circuit to provide electronic viewing to many court records…
> access to all electronic and other court records shall be governed by the Standards for
> Access to Electronic Court Records and Access Security Matrix.

Search is by case number, uniform case number, citation number, party name, or date range.
There is no listing, no index, no sitemap, and no total. Nothing to enumerate — a query
interface is not a Resource set, and hammering it with generated case numbers would be
inventing a corpus rather than observing one. It also converts a per-record access decision,
which the Access Security Matrix exists to make, into a side effect of a crawl.

**Recording the "don't" as a finding is the point.** Not every source has a collection
strategy, and a catalogue that only ever concludes *build something* will eventually build
the wrong thing.

#### The mechanism, case number → document (observed, and where it stops)

Logged as asked. This is the path a **person** takes in a browser, read off HOVER's own
front-end. It is recorded so the gate is on the record, not so it is driven.

1. **The case number comes from system C, free.** `/Civil/bulkdata/` gives `CaseNbr` per
   row — e.g. `25-CA-012120` — with the parties, judge, cause and disposition beside it. No
   search is needed to *learn* the numbers.

2. **The search page is static.** `hover.hillsclerk.com/html/case/caseSearch.html` — a
   Bootstrap tab set (Case Number, Citation, Party, Date Filed, Court Date). It carries a
   hidden `captchaToken` field and loads `js/searchByCase.js` and `js/repo.js`.

3. **A search is an AJAX POST, and a CAPTCHA gates it.** `submitSearchByCase` in
   `searchByCase.js` builds a `captchaModel` (`CaptchaCode`, `CaptchaHash`) and calls the
   data service. `repo.js` names the gate directly:
   - `dataService.isCaptchaRequired(...)` — the server decides, per request, whether a
     challenge is needed;
   - `/NewCaptcha/CaptchaCase` and `/NewCaptcha/ValidateCaptchaCase` — issue and verify;
   - a Google reCAPTCHA v3 site key is present (`grecaptcha.execute('6Le…')`).

   Some client reCAPTCHA calls are commented out, but `isCaptchaRequired` and the
   `/NewCaptcha/*` endpoints are live and **server-side**. So the gate is not a front-end
   decoration a caller can skip; the server withholds results until it validates.

4. **Results → detail is the last hop.** A successful search returns `searchResults.html`,
   and a row opens the case detail. The document images hang off the detail view.

**Where the trace stops, and why.** Enumerating the case-service endpoints past this point —
the exact result and detail-image calls — is where *documenting the mechanism* becomes
*assembling a client for it*. The line for this project sits there: the record names the
gate and the shape of the flow; it does not map the calls needed to defeat the gate. The
`isCaptchaRequired` / `/NewCaptcha/*` control is the clerk's, placed on purpose, and going
through it is out of scope regardless of the value of the records behind it.

**The consequence for collection.** The whole-document path is the clerk's **bulk-records
request**, not this search box. Two independent reasons, both already in this file: the
search is CAPTCHA-gated by design (above), and the documents are scanned images that need
OCR — ticket #12, not built — so even reached, they would enter the corpus as text-less
PDFs. The bulk request returns full documents in bulk and sidesteps both.

### System C — the answer, and it was two clicks from HOVER's front page

HOVER links **Public Data Files** → `publicrec.hillsclerk.com`, an open IIS directory
listing. No authentication, no robots.txt (404), plain `<A HREF>` links, one file per row
with size and date.

| Directory | Files | Size | Kinds |
|---|---|---|---|
| `/Traffic/Civil_Traffic_Name_Index_files/` | 24 | **1,671 MB** | csv |
| `/Criminal/name_index/hccc1020/` | 3 | **1,815 MB** | `.WP` fixed-width |
| `/Civil/alpha_index/County/` | 28 | 1,021 MB | txt, pdf |
| `/Civil/alpha_index/Circuit/` | 28 | 455 MB | txt, pdf |
| `/Traffic/Criminal_Traffic_Name_Index_files/` | 25 | 330 MB | csv |
| `/Civil/bulkdata/` | 73 | 201 MB | csv |
| `/Criminal/name_index/{Circuit,County}/` | 55 | ~600 MB | txt |
| `/Criminal/court_calendars/{Felony,MISD,Traffic}/` | 944 | — | pdf |
| `/Criminal/sentencing_guidelines/` | 30 | ~500 MB | zip |
| `/OfficialRecords/DailyIndexes/` | 142 | 21 MB | `.29` pipe-delimited |
| `/{Civil,Criminal,Probate}/dailyfilings/`, `/DailyNewCaseFilings/` | 148 | 13 MB | csv |

Roughly **6 GB**, refreshed daily — `/Criminal/dailyfilings/` was written at 01:00 the
morning it was read. The clerk even ships documentation: `index.txt` describes the
subdirectories, and `readme.txt` gives the record layout, field by field.

**Enumeration is a plain recursive directory walk.** No POST, no token, no view state, no
cap, no JavaScript. After three entries of increasingly baroque discovery, the richest source
in the file is an Apache-style index from 2014.

### Fault 1 — every CSV is unextractable, and CSV is the largest category

Live, reproducible, and it silently kills the biggest thing here:

```
centinel check https://publicrec.hillsclerk.com/Probate/dailyfilings/ProbateFiling_20260806.csv
  declared_type : application/octet-stream
  kind          : other
  chars         : 0
  unextractable : no reader for content kind `other`
```

IIS serves `.csv` as `application/octet-stream`. What each extension gets:

| Extension | Declared type | Kind | Reader |
|---|---|---|---|
| `.csv` | **application/octet-stream** | `other` | **none** |
| `.txt`, `.29`, `.WP` | text/plain | `text` | passthrough |
| `.pdf` | application/pdf | `pdf` | pdf-inspector |

`CONTEXT.md` predicted exactly this under **Declared vs inferred type**: for formats whose
first bytes are ordinary text, magic bytes alone reach `other` and no extractor claims them.
The rule that a fetched file never consults its extension is deliberate and right — a
filename's opinion must not sit where a server's header belongs. But this server's header is
`octet-stream`, which is not an opinion about the content at all; it is IIS's default for an
extension missing from its MIME map.

The cost: **over 300 CSV files, more than 2.2 GB**, collected and every one recorded
Underivable. The single largest category on the server, lost in silence.

**The lever:** when a declared type is `application/octet-stream` — the one value that
asserts nothing — fall back to the inferred type rather than treating it as evidence. That
is narrower than "trust extensions", keeps the invariant intact for every server that
declares something real, and generalises to every file server, S3 bucket and open data
portal that hands out `octet-stream` by default.

#### And the other half: both signals honest, both wrong

The reverse case is on the same server, and no header rule reaches it.
`CircuitCriminalNameIndex_A.txt` is 29 MB, named `.txt`, served `text/plain`. Its first line
is a header row:

```
Court Type|Business Name|Last Name|First Name|Middle Name|Suffix|Party Connection Type|Uniform Case Number|…
```

31 pipes, no commas. It is delimited data **with column names**, and both the extension and
the `content-type` say prose. Neither is lying — `.txt` and `text/plain` are both true
statements about the bytes. They are simply answering a different question from the one that
matters.

So the classification question splits in two, and only the first is a format question:

| Question | Answered by |
|---|---|
| what encoding is this? | extension, `content-type`, magic bytes |
| **is this prose or records?** | **only the content** |

`octet-stream` is a header defect with a header fix. This is not: no header could have
helped, because the header is correct. The only evidence is the first line, and it happens
to be self-describing — a reader that looked would get the column names free.

Rare, as these things go. But it is the same finding as fault 2 arriving from the other
direction, and together they say the interesting axis is not *format* but **shape**.

### Fault 2 — tabular records are not prose, and the pipeline assumes prose

The larger question, and the one this entry exists to raise.

`FF1020CF.WP` is 1.13 GB. Its readme says every record is **174 characters**, fixed-width,
and carries: defendant name/alias, party ID, party code, case number, division, sex, race,
date of birth, filing date, count number, count level, charge description, disposition code,
disposition date. Plus page headings and carriage-control characters every so often.

It is served `text/plain`, so it classifies as `text` and passthrough claims it. It
"extracts" perfectly — 1.13 GB of derived text, straight into chunking.

**And that is worse than failing.** Chunk geometry is tuned for prose. A chunk drawn from
this file spans forty unrelated people. Its heading path is a page header. Embedding it
produces a vector that means nothing, because the chunk is not *about* anything. The
`.29` Official Records indexes are the same shape one size down —
`DDA|29|2026206861|ORD|ORDER||||||1|06/01/2026|07:52` — and so are the 2.2 GB of CSVs,
once fault 1 is fixed and they start extracting.

So fixing fault 1 makes this worse before it makes it better: it converts 2.2 GB of honest
`Underivable` into 2.2 GB of meaningless chunks and vectors, at real embedding cost.

This is the **content type** question from `CONTEXT.md`'s framing, and the honest answer is
that a record set is not a content type at all — it is a different *shape of document*. One
CSV row is a document. One 174-character record is a document. Chunking does not apply; the
row is already the unit. That is a decision about the index, not about extraction, and it is
the first thing in this file that a reader change cannot fix.

**Recorded, not solved.** Two sightings minimum before anything gets built, and this is one.
The next open-data portal will settle whether the shape is common enough to earn a build.

### Worth deciding deliberately

`FF1020CF.WP` is a 1.13 GB flat file of named individuals with date of birth, race, sex, and
charge and disposition history, covering every Circuit and County criminal case since 1988.
It is lawfully published, openly served, and documented by the clerk — it belongs in the
catalogue and I have recorded it as collectable.

The thing to notice is that Centinel does not archive; it **builds a full-text and vector
index**. Turning that file into a person-searchable criminal-history engine is a different
act from mirroring it, and the working heuristic — *rank work by searchable text added* —
scores it first in the corpus by a wide margin. That is precisely why it should be a
decision rather than a consequence of the ranking. It is your call, and it wants making
before the collection runs, not after.

Nothing here blocks systems A or C's non-personal content: court calendars, sentencing
guideline releases, daily filing counts, registry and trust balances, foreclosure and
auction data.

### Against the three levers

| Lever | Verdict |
|---|---|
| Content kind | **wrong on `.csv`** — `octet-stream` reaches `other`, 2.2 GB unreadable |
| Reader list | correct for pdf and text. `.WP` and `.29` extract, and should not chunk. |
| Enclosure | not exercised — a directory listing, not a document |

Discovery, for once, is not at fault. System C is a recursive `<A HREF>` walk.

### What would be built

| | Lever | Generality |
|---|---|---|
| 1 | treat `application/octet-stream` as *no declaration* and fall back to the inferred type | every file server, S3 bucket and open-data portal |
| 2 | enumerate by walking an open directory listing | very common on `.gov` file servers |
| 3 | a record-shaped document: row as unit, no chunking | held at one sighting — needs a second |

Deliberately **not** built: anything that drives HOVER. See system B.

### Left on the table

- **`.zip` has a reader that cannot help it.** `sentencing_guidelines` ships 30 ZIPs at
  ~26 MB each. `ZipContainer` routes to `anydoc`, which expects a document — these are
  archives *of* documents. An archive is a container of Artifacts, which is closer to
  acquisition than to extraction, and nothing models that today.
- **The `.WP` files pass as `text/plain` by luck.** IIS's default for an unknown extension.
  A server configured differently would send `octet-stream` and hit fault 1.
- **Daily cadence and no history.** `dailyfilings` holds ~14–30 days. Files roll off. This
  is the first source in the file where *not collecting today* loses data permanently —
  which is an argument about schedule, not extraction.
- **The other 66 Florida counties.** Every one has a clerk, and Florida clerks commonly
  publish a comparable `publicrec`-style tree. If lever 2 gets built, it is worth checking
  two more counties first — a second sighting would promote the whole shape.

---

## 5. Six ordinary city and county sites — the sweep that inverted this file

*Seen 2026-08-08. `clevelandcitycouncil.gov`, `clevelandohio.gov`, `dunedin.gov`,
`boston.gov`, `medinaco.org`, `buffalony.gov`. One full run each: `investigate`, then
`source add`, then `run --skip embed --limit 50`, each into its own store.*

### The premise, tested

Entries 1–4 are exotic systems, and all four fail at **enumerate**. That agreement is what
[Left on the table](#left-on-the-table-3) proposed testing against "a county that is neither
Tampa nor Hillsborough". These six are that test: ordinary CMS sites, no viewer shells, no
postback pagers, no bulk trees. Four different platforms — Drupal, WordPress, CivicPlus,
Granicus — across five states.

The premise did not survive.

| Site | Declared | Discovered | Collected | Extracted | Reached the index |
|---|---|---|---|---|---|
| `buffalony.gov` | 579 | **0** | 0 | 0 | 0 |
| `clevelandcitycouncil.gov` | 1,309 | 1,309 | 53 | 50 | 135 chunks |
| `clevelandohio.gov` | 1,098 | 1,098 | 50 | 50 | 21 of ~100 documents |
| `dunedin.gov` | 1,625 | 1,625 | 72 | 50 | 45 of 50 documents |
| `boston.gov` | 4,260 | 4,260 | 211 | 50 | **0 of 161 PDFs** |
| `medinaco.org` | 9,939 | 9,915 | 50 | 50 | 95 chunks |

**Five of six enumerate perfectly.** Every discovered count matches the live sitemap,
checked by hand with `curl`. Medina's 24-address gap is a live site changing between two
fetches, not drift in the walk.

So the stage this file has spent four entries blaming is the one stage that works on an
ordinary site. Everything below happens **after** the addresses are correct — which is the
inversion, and it is worth stating plainly: *a site can enumerate flawlessly and still put
almost nothing in the corpus.*

### Fault 1 — `--limit` starves extract, and on one site it cost every PDF

*4 sightings of 6.* `--limit` is documented against collection only: *"Stop collection after
this many addresses, per source."* `ops/run.rs` passes the same number into `ExtractArgs`,
`TranscribeArgs` and `EmbedArgs`.

`boston.gov` shows what that costs, and the mechanism is ugly:

```
HTML pages   → boston.gov/...
PDFs         → www.boston.gov/...
```

Addresses sort alphabetically, `boston.gov` sorts before `www.boston.gov`, so a budget of 50
was spent entirely on HTML before extraction reached a single enclosure. 211 items
collected, 161 of them PDFs, and **not one PDF was read**. The report:

```
extract   50 documents · 301,798 chars
unextractable: 0
```

Re-running `extract` with no limit recovered 146 documents and 3,812,298 more characters —
including a 127-page fire prevention code that read cleanly first try. The extractor was
never the problem. Nothing in the report said anything was missing.

The other three: `clevelandcitycouncil` left 3 enclosures underived, `clevelandohio` left 60
documents, `dunedin` left 21.

### Fault 2 — the read verdict is computed, is correct, and never reaches `run`

*4 sightings of 6.* `check` prints a verdict on every read. On the Cleveland landmark pages:

```
! dom_smoothie+htmd 0.18.0+0.5.5  ·  378 of text  ·  73% link text
  ! 73% of the text is link text — 4 links in 378 chars. This is a menu, not a page.
```

`run` printed `html 135203 378ch 4ms` for the same document and moved on. A full Medina run
log contains no `!` marker at all. The measure works; only `check` shows it, and `check` is
the command an operator runs when they already suspect something.

### Fault 3 — readability picks the wrong region, or the fallback keeps everything

*4 sightings of 6, and this is the one that matches the complaint that started the work:
collecting repeatable junk pollutes the corpus.*

Two opposite failures, one cause — `MIN_READABLE_CHARS`, a fixed floor, decides between
"keep what readability chose" and "keep the whole page".

| Page | Kept | What happened |
|---|---|---|
| `clevelandohio…/czech-sokol-hall` | 378 | picked City Hall's contact block |
| `clevelandohio…/denison-cemetery` | 29,099 | found 123 chars, kept the **whole page** |
| `boston…/black-history-boston` | 614 of 191 KiB | dropped 30 named figures with bios |
| `dunedin…/After-the-Storm` | 361 | picked the site-wide emergency banner |
| `medinaco…/commissioners-meeting/2026-10-13` | 255 | dropped the pointer to the agenda |

**385 of `clevelandohio.gov`'s 1,098 addresses are that landmark template** — 35% of the
site. The live page carries `1890`, `4314 Clark Avenue`, `Architect: Andrew Mitermiler`. The
corpus carries the mayor's phone number and City Hall's office hours. `search Mitermiler`
returns nothing. The page's own title is not searchable.

And the two directions compound. The three Cleveland pages that kept the whole page supplied
**105 of that corpus's 174 chunks**. So:

```
$ centinel search police --source cleveohio -n 5
1  Denison Cemetery                0.7554  "...Recycling & Composting...Seniors..."
2  Euclid Beach Park Gateway Arch  0.7509  (the same nav menu)
3  Denison Cemetery                0.6793
4  Euclid Beach Park Gateway Arch  0.6752
5  Fine Arts Building              0.6734
```

Five chunks of navigation from a cemetery, a park arch and an office building. The Police
Division page, which extracted perfectly, is nowhere. Confident, high-scoring, and about the
wrong thing.

`black-history-boston` is the case that decides the fix. It fires **no** verdict — `✓`, 614
chars, few links, so link share cannot see it. Chars-per-KB puts it at 3.2, inside the range
`docs/STRATEGIES.md` §17 recorded for healthy pages when that measure was withdrawn. What
separates it is a number the pipeline already computes and then throws away: the fallback
path prints *"readability found only 123 chars; kept the full page instead"*. It compares
readability's yield against the whole-page yield, and then decides with a fixed floor
instead of a ratio.

### Fault 4 — a document that indexes to nothing disappears without a word

*2 sightings.* `ops/build_index.rs`:

```rust
let chunks = chunk_markdown(&stripped.text, &config);
if chunks.is_empty() {
    continue;           // no counter, no note, no line in the report
}
```

Five `dunedin.gov` pages each extracted to the same 361-character water-shortage banner and
nothing else. The boilerplate pass correctly recognised that banner as chrome and stripped
it, which left nothing, and the indexer dropped all five. They still read `✓ live` in
`list`, still report a successful extraction, and no search can reach them. That is the
50 → 45 gap in the table above.

The stripping is right. The silence is not — and it is a consequence of the boilerplate pass
itself, which is exactly the class of thing that has to be counted rather than assumed.

### Fault 5 — `investigate` and `run` disagree about the same address

*3 sightings.* On `boston.gov`, `clevelandohio.gov` and `clevelandcitycouncil.gov`,
`investigate` said:

```
recognised
  ! nothing
measured
  sitemap       none declared
  ! a lead
  no sitemap declared, so there is no surface to walk

no `source add` line: nothing here knows how to enumerate this address, so collecting it
would store a front page and little else.
```

All three serve `/sitemap.xml` at the conventional address. `run`, seconds later, collected
4,260, 1,098 and 1,309 addresses from those files.

The cause is that `crawl::Sitemap::recognise` answers `None` unless `robots.txt` *declares* a
sitemap — deliberately, so that a recognition and a fallback stay distinguishable in the
store — while `enumerate` guesses `/sitemap.xml` anyway. `run` reaches the guess through
`crawl::fallback()`. `investigate` probes only when something recognised the seed, so it
never reaches it.

The design is right and the report is wrong. This is the same gate that once made
`hillsclerk.com`'s bad reads invisible, one field over: the Lead was un-gated from
`hits.is_empty()`, the probe was left on it.

### Fault 6 — a capped probe can print a checkmark

*1 sighting, kept because the mechanism is general.* The `stopped at N addresses` warning is
written at the **top** of the walk loop, so it needs a next iteration to fire. `dunedin.gov`
declares exactly one sitemap: the loop runs once, fills past the 500 cap, the queue empties,
and no next iteration happens. `complete` is then computed by looking for that warning:

```
✓ 500 address(es) across 1 sitemaps   (probe, 25 req)
```

The real figure is 1,625. Any site whose sitemap is a single file larger than the cap gets a
confident wrong total, and the checkmark is what makes it wrong rather than merely partial.

### Fault 7 — a relative `Sitemap:` line loses the whole site

*1 sighting, kept because it is total.* `buffalony.gov`'s `robots.txt` says:

```
Sitemap: /sitemap.xml
```

A path, not the absolute URL the spec recommends — and the file is real: 200,
`application/xml`, 579 `<url>` entries, every one same-host. The declared string is pushed
into the fetch queue untouched and handed to `reqwest`, which cannot build a request from a
path with no host. 579 addresses became **0**.

The same unresolved string is then compared against the page origin to decide cross-host, so
the report also warns *"at least one sitemap is on another host"* — false, and it points an
operator away from the real cause. `collect` then said *"no discovery run for buffalo — run
`centinel discover` first"*, which is wrong twice: discovery did run, and running it again
reproduces the same empty result.

### Smaller, recorded not ranked

- **`history` shows nothing after a manual run.** *2 sightings.* `centinel history` reports
  "No runs recorded" immediately after a run that printed a full tally, while the log holds
  1.3 MB of records. The discover-to-collect ratio — 9,915 against 50 on Medina — survives
  only in terminal scrollback.
- **The boilerplate floor is one character too high for Boston's own chrome.**
  `MIN_LINE_CHARS` is 12; `Toggle Menu` is 11, so the site's most repeated navigation line is
  filtered out before it is ever counted, and it leads nearly every search snippet.
- **A shared footer document is re-fetched per page.** Dunedin fetched one `.docx` **42
  times** in a 50-page run. Content addressing means no disk was wasted; the requests were.
- **79% of `medinaco.org` is plugin stub pages.** 7,841 of 9,939 addresses are one
  auto-generated `venue/` page per distinct location string ever typed into an event, many
  near-duplicates. Worth knowing before committing to the full site.
- **One HTTP 520 that `curl` cannot reproduce.** Dunedin's `Code-of-Ordinances` fails in
  `collect` and in `check`; `curl` gets 302 → 302 → 200 to `municode.com` every time. Recorded
  as `✗ error` rather than as a refusal, which is the distinction the vocabulary exists for.

### Against the three levers

| Lever | Verdict |
|---|---|
| **Content kind** | Clean on all six. HTML, PDF, DOCX, PPTX all classified correctly, including Boston's 161 enclosures and Buffalo's `DocumentCenter` PDFs. |
| **Reader list** | **The fault line.** PDF reading is genuinely good — a 127-page code, a 15-page budget statement with real tables, an agenda with a correctly rendered meeting table, and an honest "6 pages are scans no reader here can read". HTML reading is where four of six sites lose their content, and always at the same seam: which region of the page is the document. |
| **Enclosure** | Works, and works well — Boston followed 161, Medina followed 25 agenda PDFs, `clevelandcitycouncil` picked up a `.docx`/`.pdf`/`.pptx` set. Every one of them was then starved by fault 1. |

So this entry fails **none** of the three levers as they are written, and still loses most of
its content. That is the same warning the file's preamble gives — *a site that fails none of
these but still yields nothing has failed somewhere else* — except the somewhere else is not
`enumerate` this time. It is the boundary between a reader and a report: every fault above is
either a number the pipeline computes and discards, or a limit it applies and does not
mention.

### What would be built

Nothing site-specific, and nothing new in the strategy registry. Every fault here is a
framework defect that any site triggers, which is why none of them earns a `read::Strategy`
— see that module's own doc comment. In the order the evidence ranks them:

1. **Scope `--limit` to collection**, as its help already promises.
2. **Resolve a declared sitemap against the base URL** before fetching it and before testing
   it for cross-host.
3. **Walk the fallback in `investigate`**, and label it a fallback rather than a recognition.
4. **Surface the read verdict in `run`**, and count documents that index to nothing instead
   of dropping them.
5. **Make the fallback prove it is an improvement**, instead of triggering on a length.
6. **Read the region the page marks as its content**, before guessing at one.

#### Item 5, and the ratio that did not survive being measured

The proposal was a **retention ratio** — readability's yield over the whole page's — as both
the fallback trigger and a new bad-read signal. It was measured first, on all 300 documents,
against the pages independently confirmed good and bad. It does not work:

| | ratio range |
|---|---|
| confirmed good reads | 0.101 – 0.944 |
| confirmed bad reads | 0.003 – 0.583 |

They overlap across most of their span. `buffalony.gov/362/Common-Council` is a good read at
0.101; five `medinaco.org` event pages are bad reads at 0.42–0.58. A threshold anywhere
between them costs one to keep the other. **This is chars-per-KB again**, and it was caught
the same way: by measuring before shipping rather than after.

What the measurement did settle is narrower and firmer. Only **12 of 300** documents fall
under the old character floor at all, and on eleven of them readability had found exactly the
right thing:

```
clevelandohio.gov/…/designated-landmarks/denison-cemetery     123 chars

  ## Landmark Details
  1835
  W 23rd Street and Garden Avenue
  Architect  N/A
```

That is the record the page exists to publish, and the floor was throwing it away in favour
of 29,099 characters of navigation. So the fix is not a better threshold on length — length
was never the question. **A fallback has to be an improvement**, and the test is the link
share that already survived validation: give way to the whole page unless the whole page is a
menu, and unless the short article is itself only links, which is the listing page the
fallback was built for and which still renders the CTTV table.

Measured after the change, re-deriving each store:

| Site | fell back before | after | poor reads now reported | empty documents now reported |
|---|---|---|---|---|
| `clevelandohio.gov` | 3 | **0** | 80 of 110 | 79 |
| `dunedin.gov` | 2 | 2 | 15 of 66 | 5 |
| `medinaco.org` | 0 | 0 | 50 of 50 | 0 |
| `clevelandcitycouncil.gov` | 0 | 0 | 1 of 53 | 0 |

`clevelandcitycouncil.gov` is the control: a healthy site, one flagged read, nothing lost.
`medinaco.org` gained no characters and lost none — its 50 flagged reads are the verdict
reporting a condition that was always there.

And the search that started this:

```
$ centinel search police --source cleveohio      before → 5 chunks of cemetery navigation
                                                 after  → Divisions, then Division of Police
$ centinel search "Garden Avenue"                before → 0 results
                                                 after  → Denison Cemetery: 1835, W 23rd Street
```

**What is still not fixed**, and is the honest limit of this change: on the landmark pages
where readability picks City Hall's contact block rather than the details block, the record is
still not extracted — `search Mitermiler` still returns nothing. That is the wrong-region
failure, and no length or ratio reaches it. What changed is that 79 such pages now report
themselves as producing nothing rather than filling the index with a phone number.

### The page marks its own content, and reading that is the fix

Fault 3 is the one nothing above reached: readability picking a *wrong* dense region. No
length and no ratio distinguishes 378 characters of contact panel from 378 characters of
record. The pages themselves do, and they say so in the markup.

Measured across all 300 documents, **298 carry a content marker**:

```
main          198
[role=main]   100
none            2
```

Four unrelated platforms — Drupal, WordPress, CivicPlus, Granicus — and the marker is the
HTML sectioning standard, not any of their conventions. Reading the marked region instead of
scoring for density recovers every fact that was lost, and drops the one that was wrongly
kept:

| page | fact | readability | marked |
|---|---|---|---|
| `czech-sokol-hall` | `Mitermiler`, `Clark Avenue` | absent | **present** |
| `czech-sokol-hall` | City Hall's phone number | present | **gone** |
| `black-history-boston` | `Melnea Cass`, `Reggie Lewis` | absent | **present** |
| `denison-cemetery` | `1835`, `Garden Avenue` | present | present |

**The order matters and is not obvious.** `<main>` contains `<article>` on the Cleveland
template, and the record sits *between* them — byte 112,264, where `<main>` opens at 109,469
and `<article>` at 113,024. Taking the most specific marker would miss the one fact the page
publishes. The rule is the widest region the page marks as its own.

Re-derived across five stores:

| Site | indexed before → after | empty before → after | characters |
|---|---|---|---|
| `clevelandohio.gov` | 31 → **110** | 79 → **0** | 148k → 185k |
| `boston.gov` | 191 → 191 | 0 → 0 | 3.81M → 4.26M |
| `dunedin.gov` | 61 → **66** | 5 → **0** | 235k → 354k |
| `medinaco.org` | 50 → 50 | 0 → 0 | 374k → **116k** |
| `clevelandcitycouncil.gov` | 53 → 53 | 0 → 0 | 146k → 149k |

Medina losing 69% of its characters is the point, not a loss: that was the site-wide menu on
every event page. Per-document comparison confirms no page lost its content — Dunedin's
`May-COA` went 18,701 → 1,828 characters and *kept* the date and `Hale Senior Activity
Center, 330 Douglas Avenue`, which had been buried at offset 17,700.

```
$ centinel search Mitermiler --source cleveohio
  Czech-Sokol Hall | 1890  4314 Clark Avenue  Architect Andrew Mitermiler
$ centinel search "Toggle Menu" --source boston
  0 results
```

The second one is a free consequence: dropping `nav`, `header` and `footer` *inside* the
marked region removed the chrome line that had led nearly every Boston snippet, without
touching `MIN_LINE_CHARS` at all.

**The cost, and who pays it.** A marked region is broader than readability's pick, so it
carries in-page sub-navigation: the Cleveland police page goes from 4,248 characters at 10%
link text to 12,478 at 50%, and Dunedin's flagged reads rise from 15 to 32. That is the right
trade in one direction only — **content never extracted cannot be recovered by a later
stage, and chrome that was extracted can be dropped by one.** Repeated sub-navigation is
exactly what [`crate::boilerplate`] was written for, and it is now doing that job rather than
compensating for a reader that picked the wrong region.

### The address is not content, and it was most of the corpus

`medinaco.org` still flagged 50 of 50 poor reads after the marker change, and the reason
turned out to be a defect in the measure rather than in the pages. `links_in` counts a whole
`[text](url)` span, **address included**:

```
[Google Calendar](https://www.google.com/calendar/event?action=TEMPLATE&dates=…)
 ^^^^^^^^^^^^^^^ 15 characters of content        ^^^^^^^^^^ 250 characters of address
```

Split across the derived text of five sites, what we were calling *link text* is mostly not
text at all:

| site | reported "link text" | anchor text | addresses in `<a>` | addresses in `<img>` |
|---|---|---|---|---|
| `medinaco.org` | 71% | 10% | 55% | 4% |
| `clevelandohio.gov` | 34% | 7% | 13% | 12% |
| `dunedin.gov` | 33% | 5% | 23% | 4% |
| `clevelandcitycouncil.gov` | 8% | 2% | 5% | 1% |

So an `<a>` inside a marked region is now reduced to its text and an `<img>` to its `alt`.
The words are content — *Meeting Agendas*, *Andrew Mitermiler*, *Police Lights* — and the
address beside them is machine addressing that nothing searches for and nothing should
embed.

**Nothing is lost, and that is a property of the store rather than a hope.** The derived text
is derived; every `href` stays in the raw HTML blob, which is truth and immutable. Every
consumer already reads the blob and not this text: `enclosure::documents` finds the PDFs a
page encloses from the raw bytes — that is where Boston's 161 came from — and `crumbs_on`
counts off-host hosts the same way. A crumb table rebuilds from the same blobs whenever it is
built.

| | poor reads | characters | markdown links left |
|---|---|---|---|
| `clevelandohio.gov` | 45 → **0** | 185k → 133k | 0 |
| `medinaco.org` | 50 → **0** | 116k → **28k** | 0 |
| `dunedin.gov` | 32 → **0** | 354k → 235k | 30 (PDF and unmarked readers) |
| `clevelandcitycouncil.gov` | 2 → **0** | 149k → 139k | 0 |

A Medina meeting page is now 528 characters — title, series, date, and `Administration
Building (144 N Broadway St, Medina) Balcony Room B` — where it held 10,817 of menu, and it
is searchable by venue and by series, which it never was.

**The boundary is load-bearing.** The strip applies to a marked region and nowhere else,
because stripping addresses everywhere makes a navigation menu stop *looking* like
navigation — and the test `whole_page_is_better` uses to refuse a menu reads exactly that. A
test caught it. It is also the right line on its own terms: `html_whole_page` exists so *a
listing page with no article is still content worth having*, and there the links are the
content.

**And what that leaves unanswered.** A document read from a marked region now has no
markdown links at all, so the link-share finding is structurally silent on it — the menu
question has not been answered for those documents, it has stopped being askable in those
terms. What remains are counts rather than judgements: a line repeated across the source, and
a document that produced no chunk. If a marked region full of navigation proves to be a real
shape, it needs its own measure and that measure needs validating.

### Left on the table

- ~~**A second sighting of "the extractor picked a different region than the page's own
  template implies".**~~ **Checked, and it is the answer.** See
  [The page marks its own content](#the-page-marks-its-own-content-and-reading-that-is-the-fix)
  below.
- **CivicPlus as a product recognition.** `buffalony.gov` is CivicPlus by three independent
  signals: a `frame-ancestors` CSP naming `*.civicplus.com`, the literal string in the page,
  and the `AgendaCenter`/`DocumentCenter` modules. Centinel names only `sitemap`, a standard.
  That is one sighting; a second CivicPlus city would promote it.
- **The `.org` question, answered.** `medinaco.org` is a county government on a non-government
  domain and nothing behaved differently. Recognition is evidence-based throughout; no code
  path tests the suffix. Recorded so it is not asked again.
- **Re-fetching a footer enclosure once per page.** Cheap to fix at 50 pages, not at 1,625.
  Left because it is a politeness and time cost, not a corpus cost.

---

## 6. A full run on the box — 37 Tampa Bay sources

*Seen 2026-11-12, on `ssh centinel` (VM 113, the live store). `centinel -v run` over every
enabled source, logged to `~/.centinel/run-logs/`, with RSS and CPU sampled every 2 s
beside it. v0.30.2 for the first runs, v0.31.0 from 20:54Z.*

This entry is written while the run goes. The running log at the end is appended as things
come up; the faults above it are the ones that held up.

### Fault 1 — one 830 MB file killed every run *(fixed in v0.30.3)*

Every multi-source run on the box ended in the OOM killer: 03:39 (8 sources, 3.19 GB),
17:48 (2.51 GB), 18:01 (2.57 GB), and the Nov 6 runs ended `interrupted`. CPU was idle
(40 s in 11 min); systemd stops the whole tmux scope with the killed process, so the
session vanished too.

`heaptrack` on `run --source hillsborough`, acquisition only: flat at 52 MB through
discover, then 63 MB → 367 → 876 → 1,823 MB in 13 s and killed at 2.25 GB. One call held
830 MB (`reqwest` `Response::bytes()`) and 780 MB of HTTP/2 frames held the same body.
The file: `assets.contentstack.io/…/TBAAACC_Chamber_092025_UPDATED.ppsx`, a slideshow,
`content-length: 829618617`. The 256 MiB ceiling was checked after the body was in memory.

Fixed by moving the ceiling into the fetcher (refuse a declared length before reading, and
stop reading at the ceiling otherwise), and listing what it refuses with the command that
takes it (v0.31.0). On the next run it was refused in 815 ms with 0 bytes read, and RSS
stayed at 140–280 MB through collect.

### Fault 2 — a CMS "external link" page is fetched off-host, and fails as the city's

Every 403 in the first 1,350 lines of the run was a `dunedin.gov` or `myclearwater.com`
page that redirects to a vendor: municode, OpenGov procurement, Paymentus, the property
appraiser, GovDeals, myvscloud, floridajobs, fldoe, recover.pinellas.gov,
floridahealth.gov, atozdatabases. The vendor refuses us; collect counts the city's address
as failed. None returned 200 this time, but `fetch.rs` follows redirects with no host check
and records only `final_url`, so one that did would be stored as the city's document.

**Two sites, same shape** — both are CMS link pages whose only content is the redirect.

**Way around:** stop at the first redirect that leaves the source's host, record the target
as a crumb (one Source per exact host; an off-host link is recorded, not followed), and do
not count it as a failure.

### Fault 3 — `<base href>` is ignored, and links resolve to pages that do not exist

`largo.com` pages declare `<base href="https://www.largo.com/" />` and link relatively:
`connect/mayor_and_commission/mayor.php`. We resolve against the page's own address, so a
link on `/connect/mayor_and_commission/index.php` becomes
`/connect/mayor_and_commission/connect/mayor_and_commission/mayor.php`: 404. 272 of the
run's first 362 4xx answers were this (172 in collect, 100 in discover), and any page
reachable only by those links is never found. `largo.com` also has no `robots.txt` and no
`sitemap.xml`, so the links are the whole enumeration.

By 21:55Z largo's collect stood at **57 stored, 2,451 failed** of 4,434 addresses: about
98% of what its discovery found are addresses this fault made up, compounding as each
404 page's own links (it has none, but every real page links to forty others) resolve
deeper. At one request a second that is over an hour of the run for 57 pages.

Final: **124 stored, 4,309 failed of 4,434**, collect running from 20:58 to 22:15 — 77
minutes of the run, 97% of it on addresses that do not exist.

Four resolvers each take their own base and none reads `<base>`:
`strategies/crawl/links.rs`, `strategies/crawl/listing.rs`, `enclosure.rs`, `crumbs.rs`.

**Way around:** one place says what a page's links resolve against — `<base href>`
resolved against the page, else the page — and all four use it.

*(Later: fixed. `html::Scan::base` says what a page's links resolve against — the first
`<base href>`, resolved against the page, else the page, as the HTML spec has it — and
`html::Base::join` is the one way an `href` becomes an address. All four resolvers use it,
and so do feed autodiscovery, icon discovery and the rolodex's mentions. An enclosure still
has to be on the page's own host, not the base's. Readability already read `<base>` itself.
Still open: only a quoted `href` is read, as everywhere in `Scan`.)*

### Fault 5 — a filename in a script becomes a document address (tampa, 196 × 404)

Every `tampa.gov/document/<slug>-<id>` page links its PDF correctly, and we fetch it:
`/sites/default/files/document/2025/foreclosure-registry-faqs-v2.pdf`, 200. A second later
we also ask for `/document/foreclosure-registry-faqs-v2.pdf`, which 404s. The page names
the file six times; three are bare filenames — `aria-label="Download: …pdf"`, link text,
and a download widget's `fileName: "foreclosure-registry-faqs-v2.pdf"`. The last is a quoted
string in a `<script>` ending in `.pdf`, which is exactly what `enclosure::script_targets`
takes as an address (it exists for PDFObject's `var pdfURL = "https://…pdf"`). Resolved
against the page, it lands on `/document/…pdf`. 196 requests a run, one per document page.

**Second sighting** of *enclosure detection matches JavaScript source* (entry 2: OnBase's URL
template). Way around: a string in a script counts as an address only when it looks like
one — absolute, or rooted (`/`, `./`, `../`). The `pdfURL` case is absolute.

### Fault 6 — WordPress attachment pages are a large part of a site, and they are wrappers

`madeirabeachfl.gov` discovers 4,884 addresses, and its discovery walks Yoast's
`attachment-sitemap.xml` and `attachment-sitemap2.xml` beside the post, page, event and
document sitemaps. An attachment page is the page WordPress makes for every uploaded file:
`/festival-of-lights-boat-parade/fol21-poster-11x17-lr-2/` is
`<body class="attachment … attachment-pdf">`, about 80 KB of theme around one file. At
Yoast's 1,000 per sitemap that is up to 2,000 of the 4,884 addresses, each a request at
1 per second and a page of chrome for extraction and the junk gate to deal with, while the
file it wraps is one link away.

**Second sighting** of *a plugin generates most of the site* (entry 5: `medinaco.org`'s
venue stubs), and kin to *the page is a wrapper and the content is one address away*. Way
around, not built: treat a sitemap named `attachment-sitemap*.xml` (or a page whose body
class is `attachment`) as a list of files, and take the file it wraps instead of the page.
WordPress with Yoast is a large share of small-city sites, so this is not one site's rule.

### Fault 7 — a document every page links is fetched once per page (21.5 GB for nothing)

`townofredingtonshores.com` puts five PDFs in its footer or sidebar: the FEMA substantial
damage desk reference (9.8 MB), the floodplain management plan (6.0 MB), *Evacuate or
Stay* (2.5 MB), the KDOW fact sheet and a roofing affidavit. Each was fetched **1,026
times** in this run, once for every page that encloses it, and a sixth (the town
information sheet) 593 times: about 19 GB downloaded for six files, and nearly all of
redington-shores' 2 h 17 min in collect (20:55–23:13) at one request a second.

`SiteSource::acquire` (`sources/site.rs:829`) fetches every enclosure of a page in line, and
nothing remembers that this source already fetched that address a minute ago. The blob
store dedupes the bytes; the request, the wait and the download are not deduped.

Across the run so far: **7,174 repeat fetches, 21.5 GB** — redington-shores 5,793 (19.4 GB),
st-petersburg 992 over 190 addresses (1.6 GB), south-pasadena 154, hillstaxfl 89 (at its
10 s crawl delay, 15 minutes), belleair-shore 60, gulfport 54, and four more under 15.

**Second sighting** of *a shared enclosure is re-fetched once per referring page* (entry 5:
one `dunedin.gov` `.docx`, 42 times in 50 pages).

**Way around:** an enclosure is an address, and should meet the same "already have it"
check as every address discovery hands collect. The smallest version is a per-pass set of
fetched enclosure addresses on the source; the deeper one is enclosures going back into the
frontier as resources, so they are counted, deduped and resumable like everything else.
Not checked: whether each repeat also appends its own status record to the log.

*(Later: fixed. An enclosure now meets the same "already have it" check as every address:
`acquire::Frontier`, one per pass. The first page that links a document fetches it, every
later page leaves it alone, a later pass holds it like any observed address, and `--refresh`
fetches it once more. It is still fetched inside its page's acquisition rather than queued
as an address, so the page and its documents are stored together and a resumed pass cannot
lose one. A repeat did not append a Status record, but it did append its own Observation,
crumbs-ledger row and request-log line; those stop with the fetch. Still open: a shared
enclosure that refuses once (a 503) is not retried in that pass or a later one, since every
page that links it is held and the refusal leaves no Status. The durable fix is a Status
for a refused enclosure and a work list that picks up addresses that are not live.)*

### Fault 8 — a 10 s crawl delay across a WooCommerce shop holds the whole run (hillstaxfl)

`hillstaxfl.gov`'s `robots.txt` says `Crawl-delay: 10`, and we honour it. Its discovery
reads 23 Yoast sitemaps, ten of them `product-sitemap*.xml`: a WooCommerce shop of student
artwork, one page per student (`/product/carlee-k/`, *"Carlee K. - Hillsborough County Tax
Collector"*, 250 KB, *"The product images shown are samples"*). Discovery found 10,039
addresses; collect had 5,459 left to fetch. At 00:04Z it was 867 in, ~10 s each, so the
rest is about **13 hours**, and every other source finished collect at 23:13Z. Extract,
index and embed wait on it.

Also seen: off-host enclosures (`flhsmv.gov/pdf/proc/tl/*.pdf`, 75 of them) are paced at
hillstaxfl's 10 s, because the pace is the source's, not the host's; and its `/form/` pages
return 403 to us (10 so far). Respected.

**Third sighting** of *a plugin generates most of the site* (entry 5's venue stubs, Fault 6's
attachment pages). Way around, not built: a Yoast sitemap's name says what it lists —
`product-`, `product_tag-`, `product_cat-`, `pa_*-`, `author-`, `attachment-` — and
discovery can skip or defer the ones that list a plugin's objects rather than the site's
writing. And a crawl-delay source should be visible up front: a run that knows it holds
5,000 addresses at 10 s can say "≈14 h" before it starts, not after.

### Fault 9 — extract climbs to 2.2 GB and is killed, three minutes in

The run was stopped at 06:09Z (Ctrl-C, Fault 8) and restarted as
`run --skip discover --skip collect`, PID 224862. Extract started 06:09:36; the OOM killer
took it at 06:12:26 with 2.24 GB anon RSS, and systemd took the tmux server with it.

Not one file this time. RSS, sampled every 2 s: 826 MB at 06:10:04 (28 s in), 1,181 →
1,412 → 1,517 → 1,632 → 1,705 → 2,013 → 1,998 at 06:12:17, then killed. A ratchet: it
rises with the work and never comes back down. It had just finished clearwater (1,684 of
1,684, the last files 7.4 MB and 3.7 MB) and started dunedin (4 of 2,759). Nothing in the
log near the kill is large. The per-source logs `replay` reads are small (tampa's is the
largest at 42 MB), so the replay is not it.

Extract reads `workers()` = `available_parallelism()` = 4 documents at once, each on a
`spawn_blocking` thread holding the whole blob plus the parser's state.

**Not the allocator.** `MALLOC_ARENA_MAX=2 centinel -v extract` (06:20Z) went from 226 MB
to 1,849 MB in 15 s, the same climb. Under heaptrack, one PDF's peak heap is 519 MB against
572 MB RSS: the memory is live, not stranded. *(Later: this ruled out the arena count, not
retention. At 07:22Z jemalloc showed glibc keeping freed heap was the cause of the ratchet;
the peak of one heavy document is live, as measured here.)*

**Four vector-art PDFs at once.** The work list starts with dunedin's off-host enclosures,
and `pinellas.gov/wp-content/uploads/…` sorts together: the 2026 hurricane guide in
English, Spanish and Vietnamese (17.8–19.3 MB, 20 pages each) and the Spanish damage
prevention guide (15.8 MB). Each parsed alone through `centinel check`, peak RSS:

| PDF | bytes | pages | peak RSS | × size |
|---|---|---|---|---|
| hurricane guide, EN | 19.3 MB | 20 | 561 MB | 29× |
| hurricane guide, ES | 17.8 MB | 20 | 546 MB | 32× |
| hurricane guide, VIE | 18.9 MB | 20 | 547 MB | 30× |
| damage prevention, ES | 15.8 MB | 97 | 671 MB | 44× |
| welcome guide, EN | 8.9 MB | 115 | 73 MB | 8× |
| welcome guide, ES | 9.3 MB | 143 | 82 MB | 9× |
| 400-water | 7.4 MB | 30 | 73 MB | 10× |
| 811 guide | 6.9 MB | 81 | 64 MB | 9× |
| a 1-page form | 0.2 MB | 1 | 49 MB | — |

Four of the first kind in flight is ~2.3 GB, which is the kill. Size does not predict it:
8–10× for most PDFs, 30–44× for these.

**Where it goes (heaptrack, the EN guide):** 254 MB over 962k allocations in `lopdf`'s
content-stream `operand` parser and 218 MB over 343k in the `Vec<Operation>` it grows,
called from `pdf_inspector::extractor::xobjects::extract_form_xobject_text_inner` →
`content_decode::decode_content_bounded`. A Form XObject holding vector artwork (the
evacuation map) has ~960,000 drawing operators, and `Content::decode` materialises every
one, each with heap-allocated operands, to find the text among them. pdf-inspector bounds
this by **count** — `MAX_PAGE_OPERATIONS = 1_000_000`, a `pub(crate) const` — and this
stream sits just under it, so the bound permits ~500 MB per stream. Not configurable from
centinel.

These four were derived in the 06:20Z test before it was stopped, so the next pass skips
them; the shape will recur on any vector-heavy PDF.

**Way around:**
1. Extract admits documents against a memory budget, not a core count. Since size does not
   predict cost, the simple form is weighted permits: a large document (say ≥ 5 MB) takes
   every reader slot and parses alone, small ones run `workers()`-wide. Worst case is one
   heavy document plus nothing, ~700 MB here, instead of four.
2. Upstream, in pdf-inspector: text extraction from a form does not need path operators;
   a scan that skips them (it already has an allocation-free tokenizer in
   `count_content_operators`) would make the cost proportional to text, not artwork.
3. Until either lands, `taskset -c 0,1 centinel …` halves the readers with no code change:
   `available_parallelism` honours the CPU affinity mask.

**Not a small-machine problem, a sizing problem.** Readers scale with cores; cost scales
with the document. 4 cores × ~600 MB dies on this 3.3 GB VM, passes unnoticed on a 16 GB
laptop, and on a 16-core box is ~10 GB of worst case. Collect has the same shape:
`HOSTS_AT_ONCE` = 16 × the 256 MiB ceiling is 4 GB, on any machine. Per-stage worker knobs
(`[extract] workers`, `[collect] hosts_at_once`) would push that arithmetic onto the
operator and still OOM on a wrong guess.

**Proposed (Ben, 2026-11-13: "flags/config for constrained machines", design not settled):**
one memory budget, not a knob per stage.

```toml
[resources]
memory = "1.5 GB"   # default: auto, a share of what the machine or cgroup has
```

with `--memory` to override it for one run. The budget lives in one place in core; every
stage that holds memory reserves against it before a unit of work and waits when it is
full, so a constrained machine runs slower instead of dying. Collect reserves a body's
declared length, which it knows before reading; extract reserves an estimate per document,
deliberately conservative because size is a poor predictor (a document over a few MB takes
most of the budget and parses near alone). Embed and classify are network-bound and keep
their `concurrency`. `doctor` prints the budget it detected and warns on a ballooned VM.
Open: the auto default's share, and whether this box sets it by hand. The upstream
pdf-inspector fix is still the true bound on a small PDF that is mostly artwork.

Also seen: RSS does not come back down after the heavy documents finish (1.85 GB held
while parsing 200 KB forms). Secondary: with a budget the peak is what matters. *(Later
that morning this turned out to be glibc keeping freed heap, not the parse — see 07:22Z in
the log. Fixed: the binary's allocator is now jemalloc, with the settings that test ran.)*

Also seen, and mine: `kill -INT` did not stop the test, because a job started with `&` from
a non-interactive shell ignores SIGINT. A standalone `extract` is not wired to the graceful
stop that `run` has; SIGTERM ended it, losing only the in-flight reads.

*(Later: built. One memory budget, `memory::Budget`: `--memory`, else `[resources] memory`,
else three quarters of the smaller of `MemTotal` (`hw.memsize`) and the tightest cgroup
`memory.max`, read once at start. The Lance caches it sizes (64 MiB metadata and ~640 MB
index here, Lance's 1 GiB / 6 GiB on a 32 GB machine) are set aside from it, and
reservations share the rest. Extract reserves 64 MiB, plus 40× a document's first 16 MiB,
the worst ratio measured, plus 8× the rest, where the measured ratio falls from 44× at
15.8 MB to 17× at 52 MB. On this VM that is 1.7 GiB of room after the caches: two of the
19 MB vector-art guides read at once (758 MB each), and a 52 MB scan (1.02 GB) reads with
little beside it. A 32 GB machine still reads sixteen scans at once. The reservation is held
by the read's blocking task, so a pass stopped by `--limit` or a cancel does not hand out
room its readers' children still hold. Collect reserves the size ceiling per request, at
most the default 256 MiB, and gives back all but a declared length, so sixteen hosts no
longer sum to 4 GB. `doctor` prints the budget, its source and the room, and warns on
`virtio_balloon`. The contained reader's `oom_score_adj` stays: the estimate is not a
measurement, and the budget cannot see `serve` beside the run. This box should set
`[resources] memory = "1.5 GiB"` while `serve` runs beside the pipeline and the balloon
stays.)*

### Fault 10 — one PDF with 17,162 font subsets stalls extract, and nothing bounds it

Plant City's *2022 Annual Comprehensive Financial Report*
(`plantcitygov.com/…/Annual Financial Reports/2022_city_of_plant_city_florida.pdf`,
32 MB, 151 pages, PScript5 → Acrobat Distiller 17) stopped two extract passes cold: the
heaptrack pass sat on it from 06:52:35 until killed at 06:59 at 100% CPU, and the jemalloc
pass reached it as its first unread document and sat there too. `pdftotext` reads the same
file in **2.7 s at 32 MB** (263 KB of text). `centinel check` on it alone: **8 min 52 s,
669 MB peak**, one core flat out, exit 0 — it does finish, 200× slower than poppler.

`pdffonts` lists **17,162 embedded fonts**: PScript5 writes a fresh subset of the same few
CID TrueType faces (`…+CIDFont+F1`, `F2`, …) per use, each with its own ToUnicode map.
heaptrack over 120 s: 3.8 M allocations from one path —
`pdf_inspector::process_document` → `tounicode::FontCMaps::from_doc_pages_inner` →
`collect_cmaps_from_fonts_inner` → `font_cmap_entry` → (lazy) `program_fallback_cmap` →
`build_simple_cmap_from_truetype` → `build_gid_to_unicode` → `glyph_text` →
`glyph_names::glyph_name_to_string`. It builds a fallback glyph→Unicode map from every
font *program*, 17k times, before reading a page, though every font carries a ToUnicode
map already. Separately, `lopdf` loading the document holds ~360 MB of objects.

Two faults in one:
- **pdf-inspector (upstream):** the program fallback is computed for fonts that have a
  ToUnicode map, and not shared across subsets of one face. Either alone would cut this
  from minutes to near nothing.
- **centinel:** extract has no per-document deadline. One document stops the pipeline
  for every source behind it, and since it never finishes it is never recorded as
  underivable, so every later pass stops on it again. A reader that has had its time
  should be abandoned, the document handed to `pdftotext` (the fallback that exists for
  PDFs pdf-inspector makes nothing of), and the verdict recorded. An in-process reader on a
  blocking thread cannot be abandoned, which argues for running the in-process readers in
  a child process with a time and memory limit — the same isolation would have contained
  Fault 9's peaks.
  *(Later: fixed in centinel. `pdf-inspector`, `anydoc` and `calamine` now run in a child
  of the binary (`extract/contained.rs`) under `extract::READ_DEADLINE`, two minutes per
  reader. Past it the child is killed, which frees its core and memory, and `pdftotext`
  takes the document; the verdict reads "pdf-inspector: stopped after 120s, past its
  deadline". A child that cannot be started stops the pass and records nothing. On Linux
  the child raises its own `oom_score_adj`, and `serve`'s unit sets `OOMPolicy=continue`, so
  a Fault 9 peak costs one read and not the run. The upstream pdf-inspector fault stands.)*

### Fault 11 — index learns chrome from every line of a data source, and is killed on publicrec

The 07:11Z run got through extract (6,929 documents, 36 sources, 0 errors, peak 999 MB
under jemalloc), then indexed 21 sources at 119 MB. On publicrec (Hillsborough clerk bulk
case-file CSVs) RSS went 814 → 1,619 MB in 13 s and the OOM killer took it at 07:43:29
(anon-rss 1.83 GB). It was still in `reading publicrec` (done=250 of 319), the
boilerplate-learning pass before anything is chunked.

`boilerplate::Learner::add` keeps every distinct line of up to `LEARN_SAMPLE` (500)
documents as an owned `String` in a `HashMap<String, usize>`. For a website that is the menu
and the footer, a few thousand lines. A CSV has no chrome: every row is distinct (the first
file has 691 lines, 690 unique), and publicrec's first 300 derived texts come to 804 MB,
the largest 42 MB. The map holds all of it plus a `String` header and a hash slot per row,
so its size is the source's text size times about two, before a single line is recognised
as chrome.

Two smaller costs come with the same pass:

- It reads the sample on every run, already-placed documents included (on purpose, so a
  resumed run strips what a fresh one would). For publicrec that is 800 MB of blob reads to
  learn a set that comes out empty.
- What a source's chrome is gets worked out again each time and is never recorded.

**Ways around**, any one of which would have held this run:

- **Count line hashes, not lines.** `HashMap<u64, usize>` keyed on a hash of the line is
  ~16 bytes a line whatever the line's length. A second pass over the few lines that
  reached `MIN_DOCUMENTS` recovers their text. This bounds memory by line count, not
  bytes, which is still millions of entries for publicrec.
- **Look where chrome is.** Navigation and footers sit at the top and bottom of a page.
  Counting only the first and last N lines of each document bounds the learner per
  document, whatever the content type.
- **Don't learn chrome for data.** A CSV derivation has no chrome to strip. The extractor
  knows the content type; the learner could skip derivations whose tool is the CSV path.
  This is the cleanest fit, but it is a special case, and the first two fix it for every
  shape.

The 21 sources indexed before publicrec were committed (window transactions), so a restart
resumes. publicrec is left out of the restart until this is fixed.

*(Later: fixed. The learner counts a `u64` hash per line, and only the first and last 1,000
countable lines of each document (`boilerplate::EDGE_LINES`): at most a million hashes a
source, whatever the content. Lines whose hash reaches `MIN_DOCUMENTS` are counted again by
their text on a second reading of the sample, and only that count makes chrome, so a hash
collision cannot strip a line. A source where nothing repeats stops after the first
reading. Pages under 2,000 countable lines are still counted whole. Still open: `holds_records`
knows `,`, tab and `;` only, so a pipe-delimited file's repeated rows can still be learned.)*

### Fault 12 — embed grows ~0.85 MB per commit to the vector table, without end

The 13:20Z restart reached embed at 13:25Z: 330,631 chunks owed, about 61 a second through
OpenRouter. `Writer::write` appends each batch as its own Lance commit (64–95 rows, a version
and a fragment each), which is what makes embed resumable. RSS rose in step with the
commits (sampled every 30 s against `_versions`):

| time | RSS | versions | available |
|---|---|---|---|
| 13:39:29 | 730 MB | 607 | 913 MB |
| 13:42:29 | 868 MB | 779 | 762 MB |
| 13:45:29 | 1,011 MB | 936 | 602 MB |
| 13:48 (stopped) | 1,214 MB peak | 1,109 | ~470 MB |

That is ~0.85 MB a commit at ~700 fragments (more as fragments grow, below), and it does not plateau. At that rate the 330k
chunks are ~4,500 commits, ~3.8 GB, on a VM with 3.3 GB. Stopped with one Ctrl-C at 13:48Z
(69,632 embedded, all committed) rather than let the OOM killer choose; the item boundary
held and the journal reads `interrupted`.

Not yet located. Nothing in `embed.rs` keeps rows after `append` returns: `owed` and `rows`
are dropped per batch. A batch of vectors is about 1 MB (4,096 dims × 4 bytes × ~70 rows),
so the growth is the size of one batch per commit. Candidates, unconfirmed: Lance's session
caches (the defaults are 1 GiB metadata, 6 GiB index, both larger than this VM), filled by
`checkout_latest` before every append, or the writer keeping something per version. A
heaptrack or jemalloc profile of a short `embed --limit` would say which.

Two more costs come from writing one commit per batch:

- 1,109 versions and ~1,100 fragments of ~70 rows each sit in a 31 GB table until
  `maintain` compacts them, which happens only when an embed *finishes*. An interrupted or
  killed embed leaves them, and `serve` searches across all of them meanwhile.
- `serve` held 551 MB RSS at 13:39Z, beside the run, on the same 3.3 GB.

**Ways around:**

- **Find the growth.** If it is the Lance session cache, open the table with a session sized
  to the machine. That is the same memory budget as Fault 9.
- **Commit less often.** Buffer rows to a few MB or a few seconds before each `append`. That
  is one-tenth the versions and fragments for the same resumability, at the cost of losing
  at most one buffer on a kill.
- **Stopgap now:** `centinel embed --limit N` in pieces, each a fresh process that ends in
  `maintain`.

**Measured on the stopgap (13:50Z, `embed --limit 40000`):** a fresh process is not a fresh
start. RSS went 231 → 752 MB in 30 s at `checking stored vectors`, before any commit, with
1,110 fragments in the table (the 13:20Z run made the same jump at the same step). Then
~0.9 MB a commit again, to 1,303 MB at 40,000 chunks (1,731 versions, 405 MB available).
`maintaining the vector table` started at 14:02:10, RSS reached 2.04 GB within three seconds,
and the OOM killer took it at 14:02:13. The 40,000 chunks are committed. The compaction
never committed: it left a 0-byte `data/.tmpyRgYfx`, and the table now has 1,734 fragments.
So the stopgap does not work on this box. Each piece adds ~600 fragments that only `maintain`
removes, and `maintain` does not fit in what the piece leaves. `serve` holds ~580 MB beside it.

**It is per commit, and each commit costs more as the table fragments.** *(An earlier
reading here, that every vector stays resident, did not survive the next three pieces.)*
Memory per commit rises with the number of fragments already in the table:

| when | fragments | MB per commit |
|---|---|---|
| 13:30Z run | ~700 | ~0.85 |
| piece 1 (14:04Z) | ~1,900 | ~1.8 |
| piece 4 (14:51Z) | ~3,000 | ~5 |

That is ~1.5 KB per fragment per commit, so memory grows with commits × fragments,
quadratically over a run. The likely holder is a copy of the whole fragment list, which is
the manifest, kept for every version written. Lance's session metadata cache would do this,
filled by the `checkout_latest` before each `append` (default 1 GiB, larger than what is
left on this VM). centinel's own side is bounded: `run` feeds through two channels of
`PIPELINE_SLACK`, `embed_with_recovery` returns its window, and `Writer::write` moves the
rows into `append` and drops them. The jump at `checking stored vectors` (~550–750 MB) is
separate, and does not grow with the fragment count.

This makes *commit less often* the main fix, not a side one. Fewer commits mean fewer
cached versions and fewer fragments for each one to carry. A session with a metadata cache
sized to the machine bounds what is left. A heap profile would confirm the holder.

### Fault 4 — the VM's memory is ballooned *(infrastructure)*

`virtio_balloon` is loaded on VM 113. During the v0.31.0 run `MemTotal` fell from at least
5.5 GB to 3.4 GB, 100 MB inside five seconds, while the run itself held 140–280 MB. The
3.3 GB the earlier OOMs happened against is likely the balloon's low point. Index and embed
work against a 31 GB vector table and want more than collect. **Way around:** a fixed
memory size for the VM in Proxmox (minimum equal to maximum).

*(Later: `doctor` warns when `virtio_balloon` is loaded, with how far `MemTotal` is below
the memory blocks the VM was given.)*

### Smaller, recorded not ranked

- **Files over the ceiling worth having.** Besides the slideshow, Hillsborough's
  *Post-Hurricane Regional Flooding Assessment — Findings and Recommendations* is 464 MiB.
  A real report; the too-large list is the way to it.
- **Hillsborough's asset CDN answers 422** for images and some assets
  (`images.contentstack.io`, `assets.contentstack.io`): 5 in the probe, 23 by stage 60.
- **`belleairbluffs.org` refuses the crawler at its home page** (403, no redirect). Respected.
- **Ctrl-C on `run … | tee` loses the end of the log.** The interrupt reaches `tee` too, so
  the "stopping at the next item boundary" line and the run summary never reach the file;
  the log ends mid-progress at 06:09:11. `tee -i` ignores the interrupt.
- **A quota 429 ends embed in three seconds.** The provider says how long to wait ("Retry
  the request after 1 sec") and embed's three attempts are spent before a token quota
  refills. A 429 with a stated wait should be waited out, a few times, before the run
  stops; it is the provider's pace, not a fault.
- **`chunks.lance.pre-lance14` is 31 GB on disk** beside the live 31 GB table, the copy kept
  for the Lance 14 upgrade. Ben's to delete once the new table has been read back.

### Running log

- 20:54Z — v0.31.0 run starts, PID 37007. RSS 200 MB.
- 21:05Z — stage 60/77 (south-pasadena · collect). RSS 183 MB, peak 278 MB. 3,351 × 200,
  362 × 4xx, 1 × 5xx, 0 errors, 2 refused for size. VM total 3,423 MB, 2,005 MB available.
- 21:09Z — RSS peaks at 712 MB (from ~200), CPU over 100% for several samples, while
  st-pete-beach, pinellas-park, plant-city and south-pasadena collect documents at once.
  Back to ~200 MB after. The ceiling bounds one body at 256 MiB, but not the sum across
  hosts walked together (`HOSTS_AT_ONCE` = 16): on this VM, 16 large documents in flight is
  still more than it has. Recorded, not yet seen to fail.
- 21:15Z — stage 68/77 (tarpon-springs · collect). RSS 199 MB, peak 718 MB. 7,886 × 200,
  1,034 × 4xx, 2 × 5xx, 0 errors. VM total 3,327 MB, 1,995 MB available. largo's
  `<base href>` 404s (Fault 3) are now 852 of the 1,034: 752 in collect, 100 in discover.
  hillsborough 49 × 404, tampa 20 × 404 — not yet looked at.
- 21:25Z — stage 71/77 (tbrpc · collect). RSS 316 MB, peak 798 MB (21:19:53). 11,180 × 200,
  1,802 × 4xx, 0 errors. largo 1,433 × 404 (Fault 3). tampa 196 × 404 = Fault 5.
  hillsborough 49 × 404 are past events (`/events/…/2024/01/10/…`) its sitemap still lists:
  the site's, not ours.
- 21:45Z — stage 74/77; tampa done, madeira-beach collecting 2,128 of 4,782 (Fault 6),
  largo still collecting and at 1,927 × 404 in collect alone — at 1 request a second, half
  an hour of the run spent on addresses Fault 3 invented. 14,089 × 200, 2,408 × 4xx,
  0 errors. RSS 159 MB, peak 798 MB. VM total 3,327 MB, 1,895 MB available.
- 21:55Z — stage 74/77. largo 57 stored, 2,451 failed, at 2,508/4,434 (Fault 3);
  madeira-beach 2,724/4,782; safety-harbor 265/338. 16,382 × 200, 2,991 × 4xx, 0 errors.
  RSS 195 MB, peak 814 MB.
- 22:05Z — stage 74/77; tbrpc done, largo and madeira-beach still collecting. 18,603 × 200,
  3,585 × 4xx (largo's still climbing), 3 × 5xx, 0 errors. RSS 187 MB, peak 814 MB.
- 22:15Z — stage 74/77; st-pete-beach done; largo, madeira-beach and redington-shores still
  collecting. 20,262 × 200, 4,187 × 4xx, 0 errors. RSS 195 MB, peak 814 MB.
- 22:25Z — stage 74/77; oldsmar done. 21,666 × 200, 4,779 × 4xx, 0 errors. RSS 201 MB.
- 22:35Z — largo done: 124 stored, 4,309 failed of 4,434. madeira-beach 4,781/4,782;
  redington-shores 641/1,030 is the last source collecting. 22,548 × 200, 5,071 × 4xx,
  0 errors. RSS 201 MB.
- 22:45Z — redington-shores still the only source collecting. 23,101 × 200, no new 4xx since
  largo finished, 0 errors. RSS 201 MB.
- 22:55Z — redington-shores still collecting (WordPress uploads, mostly PDFs). 23,657 × 200,
  0 errors. RSS 201 MB.
- 23:13Z — redington-shores done: 1,029 stored, 1 failed. Most of its time was six PDFs
  fetched once per page (Fault 7). Correction to the last three lines: it was never the only
  source left — hillstaxfl had been collecting since 20:58 at 10 s a request, too slow to
  show up in the line counts.
- 00:04Z — hillstaxfl is the only source collecting, 867 of 5,459, ~13 h to go (Fault 8).
  80,090 log lines, 61 WARN (hillstaxfl 403s on `/form/`), 0 errors. RSS 202 MB, CPU 9%.
  VM total 3,328 MB, 1,844 MB available.
- 00:22Z — hillstaxfl 975 of 5,459: 91 addresses in 16 min, 5.7 a minute, so ~13 h to the
  end of collect. 0 errors, no new WARN. RSS 201 MB. The run's OpenRouter key
  (`OPENROUTER_API_KEY`, set only in the run's own shell) answered a test embedding: 200,
  4096 dims, $20 limit, $0.001 used.
- 00:37Z — hillstaxfl 1,066 of 5,459, same 6 a minute. 0 errors, no new WARN. RSS 201 MB,
  1,845 MB available.
- 00:52Z — hillstaxfl 1,156 of 5,459. 0 errors, no new WARN. RSS 201 MB.
- 01:07Z — hillstaxfl 1,246 of 5,459. 0 errors, no new WARN. RSS 201 MB.
- 01:22Z — hillstaxfl 1,337 of 5,459. 0 errors, no new WARN. RSS 201 MB.
- 01:37Z — hillstaxfl 1,428 of 5,459. 0 errors, no new WARN. RSS 201 MB.
- 01:52Z — hillstaxfl 1,518 of 5,459. 0 errors, no new WARN. RSS 201 MB.
- 02:15Z — hillstaxfl 1,655 of 5,459. 0 errors. RSS 202 MB. Run 5 h 20 min old.
- 05:05Z — (watch was asleep 02:24–05:05) hillstaxfl 2,672 of 5,459, 11 failed. 6 a minute
  held all night; ~7.7 h to the end of collect, about 12:50Z. RSS 202 MB. The one new WARN
  is a 504 from hillstaxfl's own gateway at 04:42Z on a product page — the site's, once.
- 05:20Z — hillstaxfl 2,764 of 5,459. 0 errors. RSS 201 MB.
- 05:35Z — hillstaxfl 2,854 of 5,459. 0 errors. RSS 201 MB.
- 05:50Z — hillstaxfl 2,944 of 5,459. 0 errors. RSS 201 MB.
- 06:09Z — stopped by Ctrl-C to drop hillstaxfl (Fault 8). Final for hillstaxfl: 3,046
  stored, 11 failed, of 5,459. Peak RSS over 9 h 15 min: 814 MB, no OOM. The log has no
  summary (`tee` took the interrupt too).
- 06:09Z — `run --skip discover --skip collect` starts, PID 224862, log
  `run-20261113T0609Z.log`, sampler `usage-20261113T061004Z-pid224862.csv`.
- 06:12Z — **OOM-killed in extract** at 2.24 GB, between clearwater and dunedin (Fault 9).
- 06:20Z — `MALLOC_ARENA_MAX=2 centinel -v extract`, PID 228139: same climb to 1.85 GB in
  15 s, so not the allocator. Stopped 06:25Z (SIGTERM) after 1,582 derivations, including
  the four heavy PDFs.
- 06:25–06:35Z — `centinel check` + heaptrack on single PDFs: 30–44× size for vector-art
  PDFs, all of it in `lopdf` content parsing under pdf-inspector's form XObject text path.
- 06:33Z — restarted: `taskset -c 0,1 centinel -v run --skip discover --skip collect`, PID
  231081, in tmux session `run`, log `run-20261113T0633Z.log` (`tee -i`). The key comes from
  `centinel.service`'s environment, where the user unit sets it.
- 06:33Z — RSS 1,016 MB three seconds into extract: hillsborough's *FY 2025 OMB Cost
  Allocation Plan*, a 54 MB PDF, 1.8 M chars, 31 s to parse, alongside a 1.8 MB scan. It
  finished; RSS held at ~1.15 GB after (the freed heap kept, not returned), 630 MB
  available. With two readers it fit; with four it would have been this plus three more.
- 06:35Z — **OOM-killed again**, 2.12 GB anon, tmux with it. The *FY 2025 Full Cost
  Allocation Plan* (57 MB, 06:34:25–06:34:59) took RSS to 1,866 MB with 0 MB available; RSS
  stayed there after it finished and small documents pushed it over at 06:35:11. On this
  ballooned VM the run has ~2.1 GB, and one 50+ MB PDF costs ~1 GB of it. Both cost
  allocation plans were derived before the kill.
- 06:35Z — restarted on one reader: `taskset -c 0`, PID 232437, log `run-20261113T0635Z.log`.
- 06:36Z — the document in flight at the 06:35 kill was *THEDC Report 2026 Q4*, 52 MB,
  30 pages of scans: 867 MB alone, 24 s. One reader carried it; RSS fell back to ~700 MB.
  hillsborough's contentstack assets hold at least three 50+ MB PDFs at ~1 GB each.
- 06:43Z — **OOM-killed on one reader**, 2.05 GB anon, in indian-rocks-beach. No large
  document at the end: RSS climbed in steps (700 → 1,251 at 06:39:24 → 1,795 at 06:39:28
  → 1,947 at 06:40:29) through ordinary 1–34 MB agenda packets, then held flat at 1,947 MB
  for three minutes while parsing 200 KB agendas, with 3–16 MB available on the VM. RSS
  that never comes back down is either live memory accumulating (a leak) or freed heap
  glibc does not return; either way it starves the poppler children extract starts.
- 06:44Z — the rest of extract under heaptrack on one core, to tell those apart:
  `taskset -c 0 heaptrack centinel -v extract`, PID 235358, log
  `extract-heap-20261113T0644Z.log`, profile `heap-extract-20261113T0644Z.zst`.
- 06:45Z — the profile is lost: `heaptrack_interpret` itself was OOM-killed at 06:45:19
  (1.7 GB), one minute in. heaptrack is too heavy for a long profile on this VM. Extract
  kept going (same step pattern, 582 → 652 → 938 → 1,602 MB) until stopped with SIGTERM at
  06:59 after 6,381 derivations; its RSS after 06:45 is not evidence, the preload library
  was writing into a dead pipe.
- 07:02Z — the allocator test instead: the same one-core extract with jemalloc preloaded
  (`LD_PRELOAD=libjemalloc.so.2`, `MALLOC_CONF=background_thread:true,dirty_decay_ms:1000,
  muzzy_decay_ms:0`), PID 241079, log `extract-jemalloc-20261113T0702Z.log`. RSS that falls
  back after a heavy document means glibc was keeping freed heap; RSS that still ratchets
  means live memory is accumulating.
- 07:05Z — inconclusive: its first unread document was Plant City's 2022 financial report,
  and it sat on that at 100% CPU (Fault 10). Stopped.
- 07:06–07:10Z — `centinel check` and heaptrack on that report alone: the font-subset
  stall (Fault 10).
- 07:11Z — restarted with plant-city left out: `run --skip discover --skip collect` naming
  the other 36 enabled sources, one reader, jemalloc preloaded. PID 242969, tmux `run`,
  log `run-20261113T0711Z.log`. Also the allocator test, on the documents after it.
- 07:22Z — **jemalloc settles the ratchet: it was glibc keeping freed heap.** 11 min in,
  0 errors, no OOM, 3,635 documents through extract, now on st-petersburg (404 of 1,993).
  RSS rises to 400–850 MB on heavy documents and falls back to 100–270 MB after (peak
  848 MB), where glibc held every peak until the VM ran out. Under glibc the one-reader
  pass died at 2.05 GB on ordinary agenda packets; under jemalloc the same kind of work
  idles at ~150 MB. Fix: a global allocator that returns memory (jemalloc or mimalloc),
  one line in the binary — on its own it would have prevented the 06:43 kill; the budget
  (Fault 9) and the deadline (Fault 10) are still needed for the single heavy documents.
- 07:26Z — st-petersburg 575 of 1,993. 0 errors. RSS 163 MB, 1,563 MB available.
- 07:39Z — **extract done.** 6,929 documents extracted across 36 sources, 0 errors, peak RSS
  999 MB (07:29:59), one reader, jemalloc. Index starts.
- 07:43Z — **OOM in index, on publicrec** (Fault 11). 21 sources indexed at ≤119 MB, total
  1,501,559 chunks, then the chrome learner held publicrec's CSV rows and went 814 → 1,619 MB
  in 13 s. Killed at 07:43:29. Embed never ran. Found at 13:17Z.
- 13:20Z — restarted with publicrec left out as well (PID 257974, `run-20261113T1320Z.log`).
  Extract had nothing left; index resumed cleanly: the 21 committed sources wrote 0 chunks,
  then redington-shores onward is new work (st-petersburg 90,408 new chunks by 875 of
  1,858). RSS 76 MB.
- 13:25Z — embed starts: 330,631 chunks owed (includes chunks embedded nowhere yet from
  earlier runs), ~61/sec. RSS climbs ~0.85 MB per commit (Fault 12).
- 13:48Z — **stopped by hand** at 1,214 MB RSS, ~470 MB available, about eight minutes from
  the OOM killer. 69,632 chunks embedded and committed; 1,109 versions, not compacted.
  Index finished for 35 sources (all but plant-city and publicrec).
- 13:50Z — `embed --limit 40000`, fresh process, PID 264982, tmux `embed`,
  `embed-20261113T1350Z.log`. 70/sec. 40,000 embedded by 14:02:10.
- 14:02Z — **OOM in `maintain`** (Fault 12, measured on the stopgap). 1.3 → 2.04 GB in 3 s,
  killed at 14:02:13. Table intact at version 3092, 1,734 fragments, uncompacted. Embedded so
  far this afternoon: 109,632 of 330,631. Stopped here for Ben: the next step needs
  `serve` stopped for the compaction, or more memory for the VM.
- 14:04Z — Ben: keep embedding. A loop on the box (`/tmp/centinel-embed-loop.sh`) runs
  `embed --limit 25000` pieces and SIGTERMs each at `maintaining the vector table`. Piece 1:
  25,000, peak 1,037 MB, stopped at maintain as meant.
- 14:20Z — Ben's OpenRouter dashboard showed no activity. The run's key is the one the
  service holds (same hash), label `sk-or-v1-f80...527`. `/api/v1/key` showed $0.128 today
  and rising, about 165k chunks for 13 cents.
- 14:30Z — piece 2: 25,000, peak 1,164 MB. Then `maintain` went past 2 GB inside the loop's
  2 s poll, and the **global** OOM killer took it at 14:30:53. systemd stopped the tmux scope
  with it, which killed the loop too. Fix: each piece now runs in
  `systemd-run --user --scope -p MemoryMax=1400M -p MemorySwapMax=0`, so the cgroup kills
  only the piece and the loop goes on. Interrupted compactions leave `data/.tmp*`
  (five so far, 70 MB).
- 14:35Z — loop restarted with the cap. 159,632 embedded since 13:25Z, ~171k to go.
- 14:51, 15:06, 15:22, 15:39Z — four capped pieces, 25,000 each, every one stopped at
  `maintain` as meant; no cgroup kill. 259,632 of 330,631 embedded, ~71k to go. The table
  is at 4,080 versions and 4,081 fragments, uncompacted.
- 16:00, 16:15Z — two more pieces of 25,000.
- 16:31Z — the next piece **failed on a 429** after 20,608 of 20,999: the provider behind
  OpenRouter (Nebius) answered "You exceeded your tokens quota for Qwen3-Embedding-8B. Retry
  the request after 1 sec." Embed retried three times inside about three seconds and gave
  up; the loop read it as the last piece. 330,240 of 330,631 embedded, 391 left. Committed
  rows were kept, as designed.
- 00:14Z — one more piece started for the 391. 00:16Z: the same 429, 0 embedded. Eight
  hours on, the quota has not refilled, so it is a longer quota than the "1 sec" says, or
  the provider's per-key allowance for the day. Stopped there: 391 chunks owed.

---

## 7. Council agendas and minutes — the vendor platforms Tampa Bay publishes on

*Seen 2026-11-20, with `investigate`/`check` v0.32.0 on the box and plain HTTP requests.*

An investigation into Tampa's Water Street Phase 2 found no city council material in the
corpus. Nearly every city here keeps its agendas and minutes on a vendor platform, and none
was collected. Of 4,422 off-host hosts the corpus links to, the agenda platforms are:

| Platform | Clients seen | Collected by |
|---|---|---|
| Hyland OnBase Agenda Online | Tampa; Manatee and Osceola Counties on their own servers | `onbase-agenda` |
| Legistar | Hillsborough County, Pinellas County, Clearwater, Belleair | `legistar` |
| Granicus | Treasure Island (agendas); Hillsborough, Clearwater, St. Pete, Safety Harbor, Gulfport (video) | `granicus` |
| CivicWeb | Oldsmar, Madeira Beach, Largo | not built |
| CivicClerk | Tarpon Springs, Seminole, St. Pete Beach | `civicclerk` (entry 10) |

### Fault 1 — robots.txt turned the walk away, and the report said "empty"

`investigate https://treasureislandfl.granicus.com/ViewPublisher.php?view_id=1` read 231,397
characters and reported `addresses: 0, complete: true`; the agenda RSS feed beside it, the
same. It read as two bugs — scheme-relative links (`//host/AgendaViewer.php?…`) dropped, and
a feed parser that found nothing. Neither: the probe's figures said `disallowed: 1041` and
`disallowed: 1`. Every Granicus host serves

```text
User-agent: Googlebot
Disallow: /JSON.php
…
User-agent: *
Disallow: /
```

and so do the hosts Legistar keeps its documents on (`hillsboroughcounty.legistar1.com`,
`pinellas.legistar1.com`, `legistar2.granicus.com`). The Legistar Web API, `{client}.legistar.com`
and OnBase serve no robots.txt at all.

**The lever:** a walk that collects nothing because robots.txt said no has to say that, by
host, in the warnings — a figure nobody prints is the same silence. Built. And the operator
can rule, per source, that a host's rules do not bind it (`ignore_robots`), seeing what that
would collect first with `investigate --ignore-robots`. Built.

### Fault 2 — a strategy's addresses on another host were checked against the seed's rules

`Pass::keep` checks an address against `Seed::robots`, the seed host's rules. A sitemap's
cross-host entries and Legistar's `*.legistar1.com` documents were checked against a path
on the wrong host. `Walk::robots` now reads the rules of the host an address is on, once
per host, and Legistar uses it. The sitemap walk still uses the seed's — changing it would
change what every existing source collects, and that wants its own decision.

### OnBase, three months on

Entry 2's findings, rechecked:

- **The search is a GET now**, the same one the page's own form builds:
  `Meetings/Search?dropid=11&dropsv=MM/DD/YYYY&dropev=MM/DD/YYYY`. No token, no cookie.
- **The cap now says so.** All of 2024: 100 meetings, `"WarningOccurred":true,
  "WarningCode":"SEARCHLIMITHIT"`. Its halves: 77 and 57, no warning. All of 2015: 100 and
  the warning. So the walk halves a capped date range until no answer is capped.
- **`doctype` / `documentType` is the product's `PublishDocType`:** 1 agenda, 2 minutes,
  3 summary, 5 agenda packet, 6 minutes packet. (A note handed over with this work said 2
  was the agenda; meeting 2936's `doctype=2` is its proposed minutes.)
- **An unpublished document answers 200 `application/pdf` with 3,505 bytes of HTML**
  (meeting 3007, agenda). Only the meeting's `Is…Available` flags are safe to build an
  address from. An unknown meeting id answers 302 to the error page, which the fetcher
  already refuses.
- `DownloadFileBytes/x.pdf?documentType=2&meetingId=2936` and `…/2936.pdf?…` return the same
  248,827 bytes: the name is still decorative, so it is written as the meeting id.
- Summaries exist now: 12 of 61 Tampa meetings in the last 90 days, 11 of 24 at Manatee.

### Legistar

- `GET https://webapi.legistar.com/v1/{client}/events` answered without a token for all
  four clients. OData `$filter`, `$orderby`, `$top` (1,000 at most) and `$skip` work, and
  form-encoded names (`%24filter`, `+` for space) are accepted.
- Hillsborough: 9 meetings in the 90 days to 2026-11-20, one of them with 51 items and
  80 matter attachments.
- `nyc` answers `403 Token is required` with an empty body; `chicago` answers 500
  "LegistarConnectionString setting is not set up" — a client name the API does not know.
- The agendas the API names are on robots-disallowed hosts. The same PDFs are served by
  `{client}.legistar.com/View.ashx?M=A&ID=…&GUID=…`, which no robots.txt covers, but those
  ids are InSite's, not the API's (`View.ashx?M=A&ID=1398&GUID=<EventGuid>` redirects to
  `Confirmation.aspx?M1=Gone`), and are only on each meeting's `MeetingDetail.aspx`.

### Granicus

- Treasure Island's `ViewPublisher.php` links 827 `AgendaViewer.php` and 31
  `MinutesViewer.php` addresses; `AgendaViewer.php?view_id=1&clip_id=1042` redirects to a PDF
  on `granicus_production_attachments.s3.amazonaws.com`. `links` would fetch every one of
  those PDFs during discovery to learn its address, so `granicus` reads them off the page.
- Hillsborough's `AgendaViewer.php` answers 200 "This agenda is not currently published":
  its agendas are in Legistar.

### Left on the table

- CivicWeb and CivicClerk, six cities between them.
- OnBase's executed documents at `/251publicaccess/`.
- Legistar's `View.ashx` route, which would collect the documents without a ruling at one
  extra request per meeting.
- The meeting's name as the document's title (entry 2, lever 6): the search and the API
  both carry it, and a Resource still has nowhere to put it.

### Running log

- 2026-11-20 — strategies written; the live runs against throwaway stores are recorded in
  the pull request.

---

## 8. Agenda item attachments — what a council actually voted on

*Seen 2026-11-21, with plain HTTP requests against Tampa, Manatee and Osceola's OnBase, and
Hillsborough's Legistar Web API and the InSite sites of Hillsborough, Pinellas, Clearwater
and Belleair, and with the box's `log/*/source.toml` and run logs.*

Researching Water Street Tampa Phase 2 found four council approvals — the Phase 3A
Infrastructure Development Agreement (Res. 2022-464), the Subphase 3A Subdivision Agreement
(Res. 2026-365), a maintenance agreement (Res. 2026-366) and a Multi-Modal Transportation
Impact Fee Credit agreement (Res. 2026-494) — and the corpus held only their titles and
votes. The agreements are attached to agenda items, and nothing collected an item's
attachments.

### OnBase: three requests down from the meeting

Tampa meeting 2824, May 7, 2026:

1. `Documents/ViewAgenda?meetingId=2824&doctype=1` — 447 KB of HTML. Items are
   `javascript:loadAgendaItem(<id>,false)`, sections `…,true)`: 119 items and 30 sections.
   `Meetings/ViewMeeting?id=2824` builds the same address with `&type=agenda`, which the
   server does not need.
2. `Meetings/ViewMeetingAgendaItem?meetingId=2824&itemId=23596&isSection=false&type=agenda`
   — 6 KB, a "Supporting Documents" list of anchors:
   `/251agendaonline/Documents/DownloadFile/<name>.pdf?documentType=1&meetingId=2824&itemId=23596&publishId=192328&isSection=False&isAttachment=True`.
3. That address answers 200 `text/html`, a 1,448-byte "Downloading, Please wait…" page whose
   script swaps in `DownloadFileBytes`. With the same query, `DownloadFileBytes` answers 200
   `application/pdf`, 493,937 bytes: *RESO AND AGMT - SUBDIVISION FPT-24-0000003 - WATER
   STREET 3A*, the Subphase 3A agreement. No cookie, no referer.

Which keys decide the file, measured on that one (`DownloadFileBytes/x.pdf?…`):

| Query | Answer |
|---|---|
| all six keys, any file name | the agreement, 493,937 bytes, same hash |
| `documentType`, `meetingId`, `itemId`, `publishId` | the agreement |
| without `isSection` or without `isAttachment` | the agreement |
| without `itemId` | **the meeting's agenda**, 287,563 bytes |
| without `documentType`, or without `meetingId` | 302 to the error page |
| `publishId=999999999`, or another item's `itemId` | 302 to the error page |
| `documentType=3` with the four keys | 3,505 bytes: the "not published" page typed `application/pdf` |

So the name is decorative here too, the two flags are, and the four keys are the address.
Dropping `itemId` is the trap: it returns a real PDF, the wrong one. The walk writes
`DownloadFileBytes/<publishId>.pdf?documentType=1&meetingId=…&itemId=…&publishId=…`, read
from the anchor and never assembled from guesses.

**Same product at all three.** Manatee meeting 763: 45 items, 39 sections; item 66526 had
four documents (the SWFWMD well-permitting agreement, 1,209,982 bytes through
`DownloadFileBytes`, `DownloadFile` again the 1,454-byte wait page). Osceola meeting 2048:
63 items, 34 sections; items 45352, 45387, 45393 and 45007 had 4, 3, 5 and 2 documents,
each led by an "Agenda Item Summary Sheet". Neither county serves a robots.txt (both 404),
nor does Tampa's host.

**Sections carry nothing.** Four Tampa sections, three Manatee and three Osceola: no
`DownloadFile` link on any. The walk does not ask for them, which saves a fifth of the
item requests.

**No cheaper route found.** Neither `ViewMeeting` nor `ViewAgenda` names a `publishId`;
the item page is the only place one appears. Packets (doc types 5, 6) would hold the same
documents in one file, but Tampa publishes none.

**Items start in 2024 at Tampa.** Before January 2024 every Tampa agenda is a Word export
uploaded whole: `ViewAgenda` holds no `loadAgendaItem` at all (Council Regular, June 2022 to
November 2023, sampled quarterly). The first structured agenda is meeting 117, January 11,
2024, with 89 items. So the Phase 3A agreement (Res. 2022-464, meeting 1845, June 2, 2022)
is out of this walk's reach: its agenda names no item to open. The three 2026 agreements are
in it.

### Cost, and how a walk is bounded

One request per published agenda, one per item, one per attachment fetched later. At
Tampa that is about a hundred item pages per council meeting. The walk keeps every
meeting's own documents from the search answers first, then opens meetings newest first and
finishes each before starting the next, so a request budget that runs out costs the oldest
attachments and never an agenda or minutes. The seed bounds the date range
(`Meetings/Search?dropid=11&dropsv=…&dropev=…`), and `--max-sitemaps` bounds the requests
inside it.

Measured at Tampa on 2026-11-21 (all at the box's 1 request a second):

| Walk | Discover requests | Attachments to collect |
|---|---|---|
| May 2026 (search seed) | 17 agendas + 232 items = **249**, about 4 minutes | about 930 |
| Everything (front door) | 79 searches + 2,443 agendas + 6,659 items = **9,181**, about 2.6 hours | about 26,600, about 7.4 hours |

The 6,659 items are in 251 of the 348 agendas published since January 2024; the other
2,095 agendas are older uploads with none, each still one request. Attachments are 4.0 per
item over a 40-item sample, so that column is an estimate. Code Enforcement agendas carry
no items. A full backfill is about ten hours of one host's time.

That second bound matters for scheduled runs: `centinel run` discovers with the default
200-request budget. A front-door Tampa source spends most of that on the search, so a
weekly run collects the newest meeting or two's attachments and says `truncated`. A
backfill is a `discover --max-sitemaps N` over a search seed.

### Legistar: turned away at the API's door, let in at InSite's

`legistar` read `events/{id}/eventitems?…&Attachments=1` and kept every
`MatterAttachmentHyperlink`. Hillsborough meeting 1398 (Nov 16, 2026): 51 items, 80
attachments, e.g. `hillsboroughcounty.legistar1.com/hillsboroughcounty/attachments/ae602428-….pdf`
(229,516 bytes, `application/pdf`). Every one is on a `*.legistar1.com` host (Clearwater's
on `legistar2.granicus.com`) whose robots.txt is `User-agent: *` / `Disallow: /`, and none
of the box's four Legistar sources sets `ignore_robots`. Their logs hold **no**
`legistar1.com` address — no attachment, and no agenda or minutes either.

The API's ids open nothing on InSite: `View.ashx?M=F&ID=<MatterAttachmentId>&GUID=<MatterAttachmentGuid>`
redirects to `Confirmation.aspx?M1=Gone`, and `LegislationDetail.aspx?ID=<MatterId>&GUID=<MatterGuid>`
answers 19 bytes ("Invalid parameters!"). InSite's ids are its own, as entry 7 found for
meetings. But the API hands over the door: **every event carries `EventInSiteURL`**,
`https://hillsboroughcounty.legistar.com/MeetingDetail.aspx?LEGID=1398&GID=761&G=…` — 269 of
269 at Hillsborough back to 2020, and 40 of 40 recent at Pinellas, Clearwater and Belleair.
None of the four `{client}.legistar.com` hosts serves a robots.txt (404). From there it is
two pages down, measured on 2026-11-21 at one request a second:

1. `MeetingDetail.aspx` (228–265 KB) links the meeting's documents —
   `View.ashx?M=A` agenda (PDF), `M=M` minutes (PDF), `M=AADA` and `M=MADA` the same two
   as accessible HTML (595 and 616 KB against 208 and 199 KB of PDF), `M=E1` a "Meeting
   Extra" (a change sheet, PDF), `M=AO` the meeting's own attachments (a QR code, JPEG) —
   and one `LegislationDetail.aspx?ID=…&GUID=…` per matter on the agenda: 32 for 1398's 32
   matters, 6 for Pinellas 10174's 6, 17 for Clearwater 3158's 17.
2. `LegislationDetail.aspx` (92–125 KB) has an Attachments table of raw-ampersand links,
   `View.ashx?M=F&ID=15691876&GUID=A1DDDE62-…`. The counts match the API's, matter for
   matter: MM 26-0546 6 and 6, RZ-PD 26-0706 5 and 5, Clearwater ID#26-1339 1 and 1,
   ID#26-1145 0 and 0. Each answered 200 `application/pdf` (199,737 to 337,299 bytes).

**Nowhere else.** The meeting page links no `M=F`; an item's `HistoryDetail.aspx` links no
`View.ashx` at all. A legislation page also links the other meetings its matter was heard
at (RZ-PD 26-0706: five) and a `ViewReport.ashx?M=R&N=Text` report; neither is an
attachment.

Which keys decide the file, on attachment 15494315 (`View.ashx?…`):

| Query | Answer |
|---|---|
| `M`, `ID`, `GUID`, `G` | 200 `application/pdf`, 199,737 bytes |
| `M`, `ID`, `GUID` | the same bytes |
| `GUID` lowercased | the same bytes |
| without `GUID`, or without `ID` | 302 to `Confirmation.aspx?M1=Gone` |
| without `M` | 410, 35 bytes |
| `GUID` one digit off, or `ID` one off | 302 to `…M1=Gone` |
| `M=A` with the attachment's `ID` and `GUID` | 302 to `…M1=Gone` |

`M=AO` without its `N=` (the name, base64) returned the same JPEG. So `M`, `ID` and `GUID`
are the address, and `G` and `N` are decoration — a page fetched without `G` links its
attachments without it. Unlike OnBase, no partial query returns the wrong file.
`LegislationDetail.aspx` needs `ID` and `GUID` (alone, `ID` answers the 19 bytes);
`MeetingDetail.aspx?LEGID=…&GID=…` serves without `G`.

**What `legistar` does now:** reads every page of meetings, keeping each meeting's
`eventitems` address (the votes) without reading it; then opens meetings newest first, each
`EventInSiteURL` page followed by its legislation pages; keeps every `View.ashx` link cut to
`M`, `ID`, `GUID`, except `AADA` and `MADA`. It no longer keeps `EventAgendaFile`,
`EventMinutesFile` or `MatterAttachmentHyperlink`: InSite serves those documents, and a
ruled source would have held each twice. Every request is checked against its own host's
robots.txt before it is made.

**Cost, Hillsborough, at one request a second.** 269 meetings since 2020, about 3.3 a month.
Thirteen meetings sampled evenly from the API: 22.0 matters and 62.7 attachments a meeting,
2.85 attachments a matter, 6 of 286 matters with none.

| Walk | Discover requests | Attachments to collect |
|---|---|---|
| A month | 1 page of meetings + 3 meeting pages + about 66 legislation = **about 70**, about 1.2 minutes | about 190, about 3 minutes |
| Everything | 1 + 269 + about 5,900 = **about 6,200**, about 1.7 hours | about 16,900, about 4.7 hours |

Both are ceilings: a matter heard at several meetings is read once, and RZ-PD 26-0706 was
heard at five. The old walk spent 270 requests on everything and collected no document. The
default 200-request budget of a scheduled run reaches the newest eight or so meetings. The
other clients are bigger: Pinellas lists 8,845 events back to 1912 (51 in 2025), Clearwater
1,635 since 2014 (144 in 2025).

### Granicus and CivicPlus Agenda Center

`granicus` keeps each `AgendaViewer.php` and `MinutesViewer.php` off the publisher page.
Every `*.granicus.com` host serves `User-agent: *` / `Disallow: /`, so it collects only for a
source that rules `ignore_robots`; Ben ruled it for these measurements and for Safety
Harbor's source.

A client that builds its agendas in Granicus's **agenda manager** answers `AgendaViewer.php`
with a 302 to `GeneratedAgendaViewer.php` at the same query: the agenda as HTML, each item's
documents linked as `MetaViewer.php?view_id=V&clip_id=C&meta_id=M`. Measured 2026-11-22:

- **Safety Harbor** (`safetyharbor.granicus.com/ViewPublisher.php?view_id=6`, 6.8 MB): 599
  agenda rows across its bodies' tables, every row dating itself in a hidden
  `<span style="display:none;">EPOCH</span>`. Six generated agendas sampled from 2023 to
  November 2026 linked 9, 15, 17, 2, 3 and 2 documents. The box held it only as a crumb
  (`GeneratedAgendaViewer.php?view_id=6&clip_id=2918`).
- **Doral**, off the box (`cityofdoral.granicus.com`, view 2, 566 agenda rows): clip 68's
  generated agenda linked 4 documents, among them the Joint Defense Agreement.
- **Treasure Island** builds none: `GeneratedAgendaViewer.php` answers a 302 to
  `panes/EmptyTemplate.php`, 299 bytes, "No Template provided."

Which keys decide a document, on Doral's meta 9770 (40,472 bytes) and confirmed on Safety
Harbor's 199147 (200,431 bytes):

| Query | Answer |
|---|---|
| `view_id`, `clip_id`, `meta_id` | the PDF |
| `view_id` empty, or missing | the same PDF, same hash |
| `clip_id` missing, or another clip's | the same PDF |
| `meta_id` alone | the same PDF |
| without `meta_id` | 200, empty |

So the walk writes `MetaViewer.php?meta_id=M`. It asks for `GeneratedAgendaViewer.php`
directly rather than following the viewer, which on an uploading client would download the
agenda PDF only to find no links; and the newest three agendas decide whether the rest are
opened, so Treasure Island's 827 cost three requests and not a truncated run.

Cost at 1 request a second: Safety Harbor is 599 generated agendas, about ten minutes, and
about 4,800 documents at the sample's ~8 per agenda — an estimate — about 1.3 hours.

Agenda Center is entry 9. Eight box sources reach it and hold only `/AgendaCenter` itself;
seven of those modules are empty, and the eighth, Indian Shores, builds no HTML agendas, so
it has no item attachments. Lake Wales, off the box, does: `ViewFile/Item/ITEM?fileID=FILE`
from each HTML agenda.

### Against the three levers

None. It is enumeration: the content kind is a PDF and reads; the enclosure is three
requests down and only the product's own markup says where.

### Left on the table

- A per-source request budget, so a scheduled run of an item-walking source reaches past
  the newest meetings without raising every source's budget.
- Legistar's `ViewReport.ashx?M=R&N=Text`, each matter's text as a report, one more file
  per matter; and the legislation page itself, which carries the matter's history.
- Granicus clients other than Safety Harbor: Doral is the second sighting, and Clearwater,
  St. Petersburg and Dunedin show only video and boards in the crumbs.
- The item's title as the attachment's title: the item page names it ("File No.
  DEO26-23596", "Resolution approving…"), and a Resource still has nowhere to put it.

### Running log

- 2026-11-21 — endpoint shapes measured by hand at Tampa, Manatee, Osceola and
  Hillsborough; `onbase-agenda` walks items.
- 2026-11-21 — Tampa since 2024 counted: 348 agendas, 6,659 items; May 2026 is 249
  discover requests. Pre-2024 agendas name no items, so Res. 2022-464 is out of reach.
- 2026-11-21 — Legistar InSite measured at Hillsborough, Pinellas, Clearwater and Belleair:
  `EventInSiteURL` → `MeetingDetail.aspx` → `LegislationDetail.aspx` → `View.ashx?M=F`,
  keys `M`/`ID`/`GUID`; `legistar` collects through it and drops the robots-blocked API
  links. A Hillsborough month is about 70 discover requests.
- 2026-11-22 — Granicus agenda manager measured at Safety Harbor and Doral: generated
  agendas link `MetaViewer.php`, `meta_id` alone decides the file; `granicus` walks them.

## 9. CivicPlus Agenda Center — the module every CivicEngage site ships

*Seen 2026-11-21, with plain HTTP requests at ten seconds apart against the eight box hosts
and Lake Wales (`lakewalesfl.gov`), and with the box's `log/*/source.toml`, run logs and
crumbs. The store was read, not written.*

Eight box sources (Indian Shores, Oldsmar, Pinellas Park, Safety Harbor, Seminole, St. Pete
Beach, Tarpon Springs, Temple Terrace) are CivicEngage sites collected by `sitemap`. Each
holds one address in the module, `/AgendaCenter` itself, and no agenda, minutes or packet.

### Seven of the eight modules are empty

Every one of the eight answers `/AgendaCenter` with 200 and the module's page. Only Indian
Shores lists anything: 9 categories, 82 `ViewFile` links. The other seven serve the search
form with no category in it. Where their meetings are, from their crumbs and their own text:

| Source | Agenda Center | Where the meetings are |
|---|---|---|
| indian-shores | 9 categories, 2018–2026 | here |
| oldsmar | empty; "previous years … in the Document Center" | CivicWeb (`myoldsmar.civicweb.net`, 51 crumbs) |
| pinellas-park | empty; the same Document Center sentence | Document Center, by its own text |
| safety-harbor | empty | Granicus (`safetyharbor.granicus.com`, 2 crumbs) |
| seminole | empty | CivicClerk (`seminolefl.portal.civicclerk.com`, 12) |
| st-pete-beach | empty | CivicClerk (`stpetebeachfl.civicclerk.com`, 9) |
| tarpon-springs | empty | CivicClerk (`tarponspringsfl.portal.civicclerk.com`, 28) |
| temple-terrace | empty | no agenda vendor among its crumbs |

So the box's one Agenda Center source is Indian Shores. A second populated sighting came
from off the box: Lake Wales, 28 categories, 2008–2026, with HTML agendas, packets and item
attachments. Two other CivicPlus sites tried, Ocoee and DeLand, were empty or redirected
elsewhere.

### How the listing loads

The front door renders **one year per category**, that category's newest, as plain anchors.
Every other year is a tab, `javascript:changeYear(2025, 4, 'a1')`, whose script POSTs
`{year, catID, startDate, endDate, term}` to `/AgendaCenter/UpdateCategoryList` and swaps the
answer in. `UpdateCategoryList?year=2025&catID=3` as a GET answers the same fragment, but a
walk need not ask per category: the module's search is a GET.

```text
/AgendaCenter/Search/?term=&CIDs=all&startDate=01/01/2025&endDate=12/31/2025&dateRange=&dateSelector=
```

- A search **inside one calendar year** answers every category's meetings in that year on one
  page. Lake Wales 2026: the same 117 meetings, file for file, as the front door's 2026 rows.
  Lake Wales 2019: 116 meetings in 19 categories, and the city commission's 96 files equal
  what `UpdateCategoryList?year=2019&catID=4` answers.
- A search **across years** answers like the front door: each category's newest year in the
  range, the rest behind tabs. `01/01/2000`–`12/31/2027` at Indian Shores answered 41
  meetings, the front door's 41. So the walk asks one year at a time.
- No answer was paged or capped. Lake Wales' largest year, 2021, is 158 meetings on one page.
- The tabs name the years that hold meetings and no others: Lake Wales lists 2008 and
  2010–2026; 2009 is not a tab, and its search answers nothing.

### Which files, and what decides one

Each row links its files under `/AgendaCenter/ViewFile/`. Measured at Lake Wales (and the
bare id at Indian Shores too):

| Address | Answer |
|---|---|
| `Agenda/_11172026-1765` | the agenda, `application/pdf`, 71,990 bytes |
| `Agenda/_01012000-1765`, `Agenda/1765` | the same bytes |
| `Agenda/_11172026-999999` | 404 `text/html` |
| `Agenda/_11172026-1765?packet=true`, `Agenda/1765?packet=true` | the packet, 28,810,001 bytes, same hash |
| `Minutes/_11032026-1762`, `Minutes/_01012000-1762`, `Minutes/1762` | the minutes, 172,906 bytes, same hash |
| `Agenda/_11172026-1765?html=true` | the agenda as HTML, 32,631 bytes |
| `Item/18398?fileID=23880` | an item's document, `application/pdf`, 97,264 bytes |
| `Item/18398?fileID=23880&foo=1` | the same bytes |
| `Item/1?fileID=23880`, `Item/18398` | 404 |
| `Agenda/1186`, `Agenda/_07142026-1186` (Indian Shores) | the same 462,130 bytes |

The day in a file name is decoration: the id decides the meeting, and `packet=true` or
`html=true` the form. An item's document needs both its item id and `fileID`; other keys are
ignored. Every miss is a real 404, not entry 2's 200 error page. So `agenda-center` writes
`Agenda/ID`, `Agenda/ID?packet=true`, `Minutes/ID` and `Item/ITEM?fileID=FILE`: one address
per document, and a meeting moved to another day keeps it. The server ignores a `Range:`
header, so each probe of the packet cost the whole 28.8 MB.

A row also links `PreviousVersions/_MMDDYYYY-ID`, the superseded versions of an amended
agenda. Not collected.

### Per-item attachments: where the agenda was built in the module

An agenda built in the module has an HTML form, and the HTML links each item's documents
as `/AgendaCenter/ViewFile/Item/ITEM?fileID=FILE`. Lake Wales' November 17, 2026 commission
agenda: 19 items, 57 documents. Six HTML agendas sampled across years held 57, 44, 14, 0, 0
and 0 (the zeros an airport board and two workshops): about 20 each. The HTML also links the
minutes it approves on the product's other host name, `fl-lakewales.civicplus.com`, which the
walk drops as off-host.

An agenda uploaded as a finished PDF has no HTML form and no items. **Every Indian Shores
agenda is one**: no `html=true` link anywhere, so the box's one Agenda Center source has no
item attachments to collect. The packet binds the same documents into one PDF; both are kept.

### robots.txt

All nine hosts serve the same CivicPlus template: `User-agent: *` disallows `/admin`,
`/Search`, `/RSS.aspx` and similar, none under `/AgendaCenter` (`/Search` is a root prefix,
not `/AgendaCenter/Search/`), and sets **`Crawl-delay: 10`**. Baiduspider and Yandex are
disallowed outright. The pacer honours the delay, so these hosts run at ten seconds a
request, not one. The recognition says so.

### Cost, and how a walk is bounded

One request per year the seed's tabs name, then one per HTML agenda, newest first; the
listings are all read before any HTML agenda. A search seed with a range asks only the years
it touches, cut to the range; a range inside one year is the seed itself and asks nothing.
`--max-sitemaps` bounds the requests inside that.

| Walk | Discover requests | At 10 s (the hosts' delay) | At 1 req/s | Files to collect |
|---|---|---|---|---|
| Indian Shores, front door | 9 years + 0 HTML agendas = **9** | 1.5 min | 9 s | agendas and minutes only: 57 meetings and 1 minutes in 2025 |
| Lake Wales, front door | 18 years + 1,151 HTML agendas = **1,169** | 3 h 15 min | 19.5 min | 1,541 agendas, 1,149 packets, 1,236 minutes, about 22,000 attachments |
| Lake Wales, a 2026 search seed | 0 + 101 = **101** | 17 min | 1.7 min | 117 agendas, 101 packets, 105 minutes, about 2,000 attachments |

Collecting is the larger cost: Lake Wales' 3,926 meeting files are 11 hours at ten seconds,
and its attachments about 61 hours more. At the default 200-request budget a front-door walk
of Lake Wales reads every year's listing and the newest 182 HTML agendas, about a year and
three quarters of attachments, and says `truncated`.

### Which box sources would switch

One. `indian-shores` would gain a second source, `https://www.myindianshores.com/AgendaCenter`
with `strategy = "agenda-center"`, about nine requests to discover. The site source stays on
`sitemap`. The other seven have nothing in the module; their meetings want CivicWeb,
CivicClerk or Granicus (entry 7), or a Document Center walk. No box source was changed.

### Against the three levers

None. Enumeration again: the files are PDFs that read, and the only faults are that the
module lists one year at a time and a script fetches the rest.

### Left on the table

- `PreviousVersions/…`, an amended agenda's earlier versions.
- CivicPlus Document Center, where Oldsmar and Pinellas Park say their older agendas are.
- The meeting's title and category as the document's title: the row names both.
- A per-source request budget (entry 8), which Lake Wales' 1,169-request walk wants too.

### Running log

- 2026-11-21 — robots.txt and `/AgendaCenter` read at all eight box hosts; seven empty.
  Indian Shores and Lake Wales measured: year searches, file keys, item attachments, every
  Lake Wales year counted. `agenda-center` written; unbuilt and untested here.

## 15. Accela Citizen Access — permits, rezonings and site plans

*Seen 2026-11-22, with plain HTTP requests at about one a second against
`aca-prod.accela.com` (agencies `TAMPA`, `HCFL`, `PINELLAS`), Tampa's ArcGIS server
(`arcgis.tampagov.net`), Hillsborough's ArcGIS Online layer and CivicData. No account, no
login. Four POSTs in all, each the request a click on a public link makes.*

Water Street Tampa's council approvals (entry 8) name a Final Plat, an Infrastructure
Construction permit and subdivision and maintenance agreements. Those are development cases,
and Tampa keeps them in Accela Citizen Access (ACA), as do Hillsborough and Pinellas
Counties. Clearwater has an ACA agency too (`/CLEARWATER/` answers 200); not measured.
St. Petersburg does not use ACA: its permits are in Click2Gov (`stpe-egov.aspgov.com`).

| Agency | Path | Modules on the front door |
|---|---|---|
| City of Tampa | `/TAMPA/` | Building, Business, Enforcement, Licenses, Planning, RightOfWay, Utilities |
| Hillsborough County | `/HCFL/` ("HillsGovHub") | Administrative, Building, Enforcement, Fire, LandDevelopment, Licenses |
| Pinellas County | `/PINELLAS/` | Building, DRS, Planning and ten others |

Hillsborough's rezonings are not in its ACA: a "rezoning" search there answers subdivision
and site-plan records. Its RZ-PD matters are in Legistar (entry 8).

### Records are a GET

A record is `Cap/CapDetail.aspx?Module=M&TabName=M&capID1=…&capID2=…&capID3=…&agencyCode=A`.
Anonymous GET, no cookie, 200 with the whole record (Tampa VRB-26-0000129: 247,024 bytes;
Hillsborough HC-BLD-23-0043181: 311,288). The three capIDs are Accela's internal key and
cannot be derived from the record number. Three ways to get them, all GETs:

1. **An open-data layer that carries the link.** Tampa publishes two on its own ArcGIS
   server, each row with a `URL` field holding the `CapDetail` address:
   `Planning/ActiveEntitlementLocations` (273 records: 74 rezonings, 42 variances, 38
   formal decisions, 29 design exceptions, 27 special uses, 25 vacatings and others; the
   oldest created January 2021) and `Planning/PermitsAll` (2,573 permits, created 2006 to
   now). Both hold **active** records only: a record drops out when it closes, so neither
   is an archive. Hillsborough publishes
   `services.arcgis.com/apTfC6SUmnNfnxuF/…/AccelaDashBoard_MapService20211019/FeatureServer/0`,
   110,574 issued and CO'd building records from April 2019 to November 12, 2026, every one
   with an `ACA_LINK`. One search found no Pinellas layer.
2. **The record number.** `Cap/GlobalSearchResults.aspx?QueryText=FPT-24-0000003` answers 302
   to that record's `CapDetail` when exactly one record matches, at all three agencies, and
   case does not matter. An unknown number answers 200 "returned no results" (70 KB);
   a number that a second record also matches answers the result list (Pinellas DRM-25-00102:
   two rows, itself and DRM-26-00035).
3. **The global search**, a GET (`QueryText=water street`): ten rows a page, "100+" past a
   hundred, the record links and the pager all `__doPostBack`. Not a listing a walk can use.

CivicData's Tampa permit datasets (BLDS) list 11 resources, and the datastore answers "not
found" for the three tried; the datasets were last modified in July 2026.

### Documents are not addressable

The record page holds an iframe:

```text
../FileUpload/AttachmentsList.aspx?iframeid=ctl00_PlaceHolderMain_attachmentEdit&module=Planning&isInConfirm=False&isdetail=True&isaccountmanager=False&isAdmin=False&isPeopleDocument=&agencyCode=TAMPA&isForConditionDocument=N
```

It names no record. The record is whatever the session last opened:

| Request | Answer |
|---|---|
| the iframe, no cookie | 302, "Object moved" (175 bytes) |
| the iframe, with the cookies `CapDetail` set (`ACA_SS_STORE`, `.ASPXANONYMOUS`, …) | 200, the list, 51,911 bytes, five rows a page |
| a file name in the list | `javascript:__doPostBack('attachmentList$gdvAttachmentList$ctl02$lnkFileName','')` |
| that postback, POSTed with the page's `__VIEWSTATE` and the session | 200 `application/octet-stream`, `attachment; filename=site%20plan.pdf`, 4,422,591 bytes, `%PDF-1.4` |
| page 2 of the list | another postback; a new `__VIEWSTATE` |
| two downloads from page 2's one `__VIEWSTATE` | both served: 6,172,755 and 59,972 bytes |

So a document's bytes are the answer to a POST, inside a session that a GET of the record
set up, keyed by the row's position on a page of the list. The list's script builds no
download URL, and none of the twenty scripts the two pages load names a download handler.

The document **does** have a stable identity. Each row carries a `documentNo`
(`ViewDocumentDetails('TAMPA','6595308',…)`), and
`FileUpload/DocumentDetail.aspx?agencyCode=TAMPA&documentNo=6595308` answers by GET, with no
cookie: file name, size, type, upload date and the record it belongs to ("Variance Review
Board - VRB-26-0000129"). Metadata only; no bytes, no link to them.

Same at all three agencies: Tampa VRB-26-0000129, Hillsborough HC-BLD-23-0043181 (five rows,
two pages) and Pinellas TY2-22-00004 (a survey and a concept plan, 5.09 and 14.49 MB) list
their attachments publicly, each behind the same iframe and the same postback. Nothing
here needed a login.

### Water Street

The record number reaches it. Tampa FPT-24-0000003, the Phase 3A Final Plat (Complete, so in
neither active layer), has 36–40 attachments over eight list pages, among them *03 -
2023.10.04 WST Ph3A- Final Plat.pdf*, *06 - 2023.09.21 WST Ph3A OM Agreement.docx* and *07 -
2023.09.26 WST Ph3A- Subdivision Agreement.doc* — the drafts of the agreements entry 8 found
the council voting on — and a bonding cost estimate and a closure report. PLN-21-0000019,
Infrastructure Construction at 200 S Caesar St, has 26–30. Five Water St building permits are
in `PermitsAll` today. A walk seeded by the active layers would reach the open records and
miss the plat.

### Cost

One record is two GETs (record, list), one POST per extra list page, and one POST per
document. Attachment counts, from the list's first page and its pager:

| Sample | Attachments a record | List pages a record | Requests a record |
|---|---|---|---|
| Tampa, 10 records (6 from the entitlement layer, 4 by number) | 12–15 (0 to 45) | 3.1 | about 17 |
| Hillsborough, the layer's 6 newest (CO'd, mostly new construction) | 29–33 (16 to 45) | 6.7 | about 39 |

At one request a second:

| Walk | Records | Requests | Time |
|---|---|---|---|
| Tampa active entitlements | 273 | 1 layer query + about 4,800 | about 80 min |
| Tampa active permits, record pages only | 2,573 | 2 layer queries + 2,573 | about 43 min |
| Hillsborough, record pages only | 110,574 | 56 layer queries + 110,574 | about 31 h |
| Hillsborough, one month with documents | about 1,200 | about 47,000 | about 13 h |

Hillsborough's sample is six records, newest first and all CO'd, so its per-record figure is
a guess at the order of magnitude. Tampa issued about 128 rezonings in fiscal 2026
(`REZ-26-0000128` is a record and `…129` is not, by bisection, which assumes no gaps).

### robots and headers

`aca-prod.accela.com/robots.txt` is a 404, `arcgis.tampagov.net`'s too, and
`services.arcgis.com` answers 403 "Invalid URL": no rules anywhere. But every `CapDetail` and
`GlobalSearchResults` answer carries

```text
X-Robots-Tag: noindex, nofollow, noarchive, nosnippet, nocache
```

and Centinel reads no `X-Robots-Tag`. `DocumentDetail.aspx` and the ArcGIS answers carry
none; the attachment list was not checked. A record page also names the applicant, owner and
contractor, often with a phone number and an email address.

### What Centinel can do today, and what it cannot

`Walk` is one paced GET and `Fetch` is one GET; the client holds no cookie store (reqwest is
built without its `cookies` feature). A strategy could enumerate records today: page an
ArcGIS layer and keep each row's link. It could not collect one attached document, because
no address names a document's bytes. Entry 3 and entry 2 found the same wall at enumerate
("Enumerate is not a GET"); this is the same wall at **acquire**.

### What would be built

The smallest design, in order. Nothing is built yet.

| | Lever | Generality |
|---|---|---|
| 1 | a **session** in the fetch layer: one cookie jar for a sequence of requests to one host, `get` and `post_form`, through the same pacer, budget, robots check and memory budget as a GET | every WebForms site: CTTV's pager (entry 3), OnBase's old search (entry 2), Agenda Center's year tabs (entry 9), ACA |
| 2 | a pure **WebForms form reader**: the form's action and hidden fields, and the body of one `__doPostBack(target)` | the same; tested over stored pages |
| 3 | an artifact address for a document **cut from a record**: `<CapDetail URL>#document=<documentNo>`, the feed entry's rule (`<feed>#entry=<id>`) one product over | ACA; any page that serves documents only through itself |
| 4 | a strategy may say **how its documents are reached** from a page it named, given a session: ACA's GET record, GET list, POST each page, POST each document not already held | ACA first; the open question below |

The address in 3 is built from the publisher's id, never from the row index or the view
state, and `DocumentDetail.aspx?documentNo=` proves the id is the server's. The record page
stays the marker. Lever 4 is the one that wants a decision: today a strategy owns enumerate
and `addresses_are`, and nothing after. Either a strategy gains an acquisition route beside
`addresses_are`, or ACA becomes its own adapter, as `ChannelSource` is for a video's three
artifacts. The first keeps a product in one file; the second keeps strategies free of
acquisition.

With 1–4, an `accela` strategy seeded by an ArcGIS layer collects records and documents at
Tampa and Hillsborough. Pinellas has no layer found, and a walk over guessed record numbers
would invent a corpus rather than observe one (entry 4's HOVER), so it stays a seed a person
writes: one record number at a time.

### Against the three levers

None of the three. The documents are PDF, Word and RTF and would read; the content kind and
reader list are not in question, and there is no enclosure address to find. It is acquisition,
and below it the fetch layer.

### Left on the table

- Clearwater's ACA agency.
- The record page's own text — description, status, conditions, inspections — collectable by
  GET today with an ArcGIS-seeded strategy, if the documents wait.
- Pinellas: a layer, if one exists; its DRS and Planning modules hold the development cases.
- St. Petersburg's Click2Gov.
- A ruling on `X-Robots-Tag: noarchive`, which no Centinel source has met before.

### Running log

- 2026-11-22 — `TAMPA`, `HCFL`, `PINELLAS` confirmed; St. Pete is Click2Gov. robots.txt 404
  at ACA; `X-Robots-Tag: noindex, nofollow, noarchive` on record pages.
- 2026-11-22 — Tampa and Hillsborough ArcGIS layers carry `CapDetail` links (273 + 2,573
  active at Tampa, 110,574 at Hillsborough). CivicData's Tampa datastore is gone.
- 2026-11-22 — attachments: session-bound iframe, `__doPostBack` per file, bytes in the POST
  answer; `documentNo` is a stable id with a GET metadata page and no GET bytes. Same at all
  three agencies. Water Street Ph3A plat FPT-24-0000003 reached by record number: 36–40
  attachments including the subdivision and O&M agreement drafts.
- 2026-11-22 — 16 records sampled for cost. No strategy built: documents need POST and a
  session, and the fetch layer has neither. Design above.

---

## 16. Procurement records — where Tampa Bay publishes solicitations, awards and contracts

*Seen 2026-11-22. Plain HTTP requests at about one a second (five at `pcsb.org`, as its
`Crawl-delay` asks), with Centinel's User-Agent, no account and no login, plus the box's
`crumbs/*.jsonl` and `log/*` read but not written. About 70 requests in all.*

The question was where the region's solicitations (RFP, ITB, RFQ), addenda, bid
tabulations, awards and contracts are, and whether any of them can be collected. The box's
crumbs name the vendors already: 31 links to DemandStar from eleven sources, 22 to
Hillsborough's Bonfire portal, 11 to OpenGov Procurement from three, 6 to PlanetBids.

### The catalogue

| Government | Where solicitations are | Documents without a login? | robots.txt | How a record is addressed |
|---|---|---|---|---|
| Tampa (Purchasing) | OpenGov Procurement, `procurement.opengov.com/portal/cityoftampa` | not to us: Cloudflare challenge, 403 | none: `/robots.txt` answers 200 with the app's HTML shell | `/portal/{slug}/projects/{id}`, `…/document?section=all`; a client-side app |
| Tampa (Contract Administration) | `tampa.gov/rfq/…`, `tampa.gov/contract-administration/construction-bids/…` | **yes**, on `tampa.gov` | Drupal default; allows these | one Drupal page per solicitation; documents at `/sites/default/files/rfq/{docs,addenda,attached}/…` |
| Hillsborough County | Bonfire (Euna Procurement), `hillsboroughcounty.bonfirehub.com` | no: registration to download | `Disallow: /` | client-side app |
| Hillsborough County (contracts) | Term Contract Report PDF, `mediaweb.hillsboroughcounty.org/newsroom/documents/Procurement/` | **yes** | **`Disallow: /`** | an IIS directory listing of three PDFs; the report was replaced 2026-11-20 |
| Hillsborough Clerk (executed contracts) | OnBase Public Access, `publicaccess.hillsclerk.com/br199/` | **yes** | none (404) | a POSTed search; each hit's document is a GET on a token that changes per search |
| Pinellas County | OpenGov Procurement, `/portal/pinellasfl` | not to us: Cloudflare | as Tampa | as Tampa |
| Pinellas County (awards) | `pinellas.gov/awarded-contract-list`, `/awarded-contracts-list` (CCNA) | **yes** | WordPress; `/wp-content/uploads/*` allowed | one page each, linking a dated PDF on the same host |
| Pinellas Clerk | Bonfire, `mypinellasclerk.bonfirehub.com` | no | `Disallow: /` | — |
| St. Petersburg | OpenGov, `/portal/stpete` and `/portal/stpete/contracts` | not to us: Cloudflare | as Tampa | `stpete.org/business/procurement/bid_results.php` links OpenGov and holds no documents |
| Clearwater | OpenGov, `/portal/myclearwater` | not to us: Cloudflare | as Tampa | — |
| Plant City | OpenGov, `/portal/plantcitygov` | not to us: Cloudflare | as Tampa | — |
| Pasco County | Bonfire, `pascocountyfl.bonfirehub.com`, since 2022-12-12; BidNet Direct before | no: the county says registration is needed to view and download | Bonfire `Disallow: /`; BidNet allows its public pages | — |
| Manatee County | its own RFP Post, `public.mymanatee.org/rfp_post/controller`; also DemandStar and BidSync | the county says yes, no registration; **to us, a Cloudflare challenge**, 403, even for `/robots.txt` | unreadable | `controller?view=home&list=…` with a display-tag pager |
| Largo | PlanetBids, `vendors.planetbids.com/portal/84253` | no: vendor login | `/robots.txt` answers **202 with an empty body**, a WAF | client-side app |
| Tarpon Springs | Bonfire, `ctsfl.bonfirehub.com` | no | `Disallow: /` | — |
| Belleair, Dunedin, Kenneth City, Largo, Oldsmar, Pinellas Park, Redington Beach, Redington Shores, Seminole, Temple Terrace, Treasure Island | DemandStar | no: free registration to download, by the governments' own pages | `User-agent: *` and nothing else | `/app/limited/bids/{id}/details` and the old `bid_list.asp?mi=…` both answer a 2,243-byte app shell with no record in it |
| Hillsborough County Public Schools | VendorLink, `myvendorlink.com/external/bids?a=55` | **no**: the details are public, the documents are not | allows `/`, disallows `/Internal` and AI crawlers by name; `Content-signal: ai-train=no` | an ASP.NET WebForms grid; every row and every document is a `__doPostBack` |
| Manatee County Schools | VendorLink, `?a=59` | no | as above | as above |
| Pinellas County Schools | OpenGov, `/portal/pcsb`; intents to award on `pcsb.org` | OpenGov: not to us; `pcsb.org`: yes | `pcsb.org` disallows `/fs/` and allows only `/fs/pages/N` | the intents are `/fs/resource-manager/view/{uuid}`: disallowed |
| Pasco County Schools | IonWave, `pascok12.ionwave.net`; tabulations on `pasco.k12.fl.us/purchasing/page/board_recommendations/` | IonWave: registration | IonWave `Disallow: /` | the recommendations page linked no file when read |

### What the platforms do to a client that is not a browser

**OpenGov Procurement** is the region's most common platform: six governments (Tampa,
Pinellas County, Pinellas Schools, St. Petersburg, Clearwater, Plant City). Its project and
document pages are indexed by search engines, so they are public to a person. To Centinel,
every portal and document address answered **403 with Cloudflare's "Just a moment…"
challenge**, and `/robots.txt` answered 200 with the single-page app's HTML (50,246 bytes,
titled "Page Not Found"), which `Robots::parse` reads as no rules. The fetcher already files
a 403 as **Blocked**, not Gone, and that is right. Getting past the challenge would mean
posing as a browser, and that is not done. Nothing to build.

**Manatee's RFP Post** is the county's own system, and the county's page says registration
is not required. It is behind the same Cloudflare challenge, on `/robots.txt` as well.

**Bonfire and IonWave** serve `Disallow: /` to every crawler: all four Bonfire hosts
measured, and IonWave. Documents also need a login. Two requests went to these hosts
before the rules were read (see the running log).

**DemandStar** answers every record address with the same 2,243-byte shell, and the
records come from an API. Every government's page says downloads need a free account.

**VendorLink** shows the most without a login. The grid lists every solicitation back to
2020 (HCPS: at least 11 pages of 20). A View postback returns the full detail: scope, dates,
contacts, questions and answers, and a list of documents with names and types. Pressing a
document's download postback answers the same page with *"In order to download the
solicitation document, you must sign in or register online."* So the documents are gated.
The details are public, but every address in them is a POST in a WebForms session, which
no `Walk` can make.

### The one public document system: OnBase Public Access

Hyland's **Public Access Viewer** (`_obpa/obpa_app.js`, an Angular app over `api/`) is up
at two sites in the region, with no login and no robots.txt:

| Host | What its queries hold |
|---|---|
| `publicaccess.hillsclerk.com/br199/` | query 199, *PAV - Contract Certified Documents*: Hillsborough's executed agreements, contracts, modifications, work orders and change orders |
| `tampagov.hylandcloud.com/251publicaccess/` | query 104, *DOC - PAV*: Tampa's CRA resolutions, executive orders, ordinances and resolutions |

How it works, from its own script and config (`obpa-config.json`):

- `GET api/CustomQuery` and `GET api/DocumentType` list the queries and document types.
- The search is `POST api/CustomQuery/KeywordSearch` with JSON
  `{QueryID, Keywords, FromDate, ToDate, QueryLimit}`. Hillsborough's config caps
  `queryLimit` at 300. One search for 2026-11-01 to 2026-11-21 returned 202 documents with
  `"Truncated": false`: 67 work orders, 51 agreements, 41 contracts, 19 modifications and
  the rest. Each hit carries 16 columns: board doc number, supplier, bid number, amount,
  dates and department.
- A document is `GET api/Document/{ID}/?ViewerMode=PDF&OverlayMode=View`. That served a
  34-page agreement as `application/pdf`, 3,530,200 bytes.

Three facts stop this from being a strategy today:

1. **The search is a POST.** `Walk` is one paced GET, by design (`strategies::crawl`).
2. **A document's address changes with every search.** The same search run twice returned
   202 IDs each time, with **0 in common**. The IDs are encrypted tokens, not document
   numbers. A token from the first search still served the PDF after the second. So the
   same document gets a new address every run. That breaks the one rule a Resource rests
   on: one address per document. The stable name is the board doc number (`26-1129`), and
   no URL carries it.
3. **The documents are scans.** `pdffonts` lists no font, and `pdftotext` returns 34 bytes
   for 34 pages. Collected today, the corpus would gain zero searchable characters,
   because OCR is still `planned` (`CONTEXT.md`, *Need*).

Only one of the two sightings is procurement. As a product, though, it is two, and Tampa's
resolutions approve contracts too.

### What is already collected

**Tampa Contract Administration is collected now.** The `tampa` source walks `tampa.gov`'s
sitemap and the enclosure scan takes the documents: 108 `/rfq/` pages, about 353
construction-bid pages, and 640 PDFs under `/sites/default/files/rfq/` (300 `attached`, 230
`docs`, 99 `addenda`, 11 `protected`). One RFQ page,
`/rfq/26-d-00044-assesment-fire-stattion-6-9-and-10-renovation`, links one addendum and
five solicitation documents on the same host. No other government's procurement is in the
store. Hillsborough's 144 matching addresses are policy documents and impact-fee offsets,
not solicitations. Pinellas County's own site is not a box source at all.

### What the open files hold

| File | Size | Holds |
|---|---|---|
| Pinellas *Purchasing Contract Listings* PDF, report date 2026-08-18 | 1,249,441 bytes, 332 pages, text layer | 1,268 contract/vendor records: contract number, title, department, contact, vendor, amount, term |
| Pinellas *CCNA Contract List* PDF | not fetched | linked from `/awarded-contracts-list` |
| Hillsborough *Term Contract Report* PDF, replaced 2026-11-20 | 398,460 bytes, 94 pages, text layer | every term contract: agreement number, title, dates, buyer, contract manager, department, vendor, source (ITB, RFP, SS), bid number, amount, balance |

The two county lists are the regional contract database the brief asked about. They are
lists, not the contracts. The contracts themselves are Hillsborough's BR199 scans above.

### Cost

Measuring: about 70 requests and 9.1 MB, most of it one 3.5 MB contract scan and the 1.2 MB
Pinellas list.

Collecting what is open and allowed:

| Source | Discover | Collect |
|---|---|---|
| `pinellas-contracts` (`links` at the list page) | 1 page | 1 page + 1 PDF (1.2 MB) per change |
| `pinellas-ccna-contracts` | 1 page | 1 page + 1 PDF |
| `hillsborough-term-contracts` (`listing`, **needs a robots ruling**) | 1 listing | 3 PDFs, about 2 MB |

All three republish in place, so the value is in the history: each run that finds new
bytes records a new version of the region's awarded contracts.

### Against the three levers

None of them fails. Every barrier here comes before Centinel holds any bytes: a login, a
bot challenge, robots.txt, or a POST. The one public system that clears those (OnBase
Public Access) has an identity problem and an OCR problem, not an extraction problem.

### Strategy: none built

The brief asked for a strategy for the platform that serves the most governments with
public, no-login documents, if it recurs at two. None does in a form a strategy can collect:

- OpenGov's six are public to people and challenged for us. Building around that is
  evading a bot wall.
- DemandStar (12), Bonfire (4), VendorLink (2), PlanetBids (1) and IonWave (1) gate their
  documents behind a vendor account.
- OnBase Public Access is public at two sites. Collecting it needs a POST in `Walk`, a
  Resource whose identity is not its fetch URL, and OCR. Those are design changes, not
  work inside one strategy.

The design that would make OnBase Public Access collectable, offered for a decision and not
built: the strategy mints a stable address, `…/br199/api/Document?query=199&doc=26-1129`,
from the hit's board doc number, the way a feed entry is `<feed>#entry=<id>`. `acquire`
resolves it through a fresh search for that one document. That puts the POST in acquire,
not enumerate, and still needs both a POST and an OCR reader. Until OCR exists, it collects
images.

### Left on the table

- **OnBase Public Access**: the design above.
- **VendorLink's public details**: scope, dates, contacts and Q&A for every HCPS and
  Manatee Schools solicitation since 2020, as WebForms postbacks.
- **Pasco Schools' board recommendations**: tabulations, by the district's account; the
  page linked no file when read, so they may be loaded by script.
- **Bid tabulations generally**: every platform that holds them gates or walls them, except
  Tampa's addenda pages.
- **Council agendas as the award record**: every award over the threshold goes to a board
  vote, and entries 7–8 collect those agendas and their item attachments. For OpenGov
  governments, that may be the only collectable record of an award.

### Running log

- 2026-11-22 — box crumbs read for vendor hosts; robots.txt and one portal page read at
  each platform; VendorLink's detail and download postbacks pressed once; OnBase Public
  Access's two GET lists read and one BR199 search run twice; Tampa's procurement coverage
  counted in the box's log.
- 2026-11-22 — four requests went to robots-disallowed hosts before their rules were read.
  The robots fetch and the page fetch were batched in one command:
  `hillsboroughcounty.bonfirehub.com/portal/?tab=openOpportunities`,
  `pascok12.ionwave.net/`, and `mediaweb.hillsboroughcounty.org`'s Term Contract Report and
  its directory. One request each; none repeated.
- 2026-11-22 — no strategy written; two Pinellas sources ready to add, and one Hillsborough
  source that needs an `ignore_robots` ruling.

---

## 17. Public-records request portals — the requests, and what agencies released

*Seen 2026-11-22, with plain HTTP requests (about 175, at least 1.1 s apart, JustFOIA hosts
at their 10 s `Crawl-delay`), the box's crumbs, and web searches for the agencies the crumbs
did not name. No account, no login, no form submitted. The store was read, not written.*

The question: Florida agencies take public-records requests through vendor portals, and some
portals publish the requests and the files released for them. Which Tampa Bay agencies use
one, and what can be collected without signing in?

The answer is almost nothing. Two of the three vendors close their portals to crawlers, and
the agencies on the third publish no documents.

### Who uses what

30 agencies, from the box's crumbs and each agency's own public-records page. 25 run a
vendor portal:

| Agency | Platform | Host |
|---|---|---|
| City of Tampa | GovQA | `cityoftampa.govqa.us` |
| St. Petersburg | GovQA (older domain) | `stpetefl.mycusthelp.com` |
| Safety Harbor, Gulfport | GovQA | `cityofsafetyharbor.govqa.us`, `gulfportflorida.govqa.us` |
| Pinellas County | GovQA | `pinellas.govqa.us` |
| Pinellas Clerk | GovQA | `pinellascountyfl.govqa.us` |
| Pinellas Sheriff | GovQA | `pcso.govqa.us` |
| Hillsborough Sheriff | GovQA | `hillsboroughsheriff.govqa.us` |
| Pasco County | GovQA, now branded Catalis | `pascocountyfl.govqa.us` (also `pascocounty.govqa.us`) |
| Pasco Sheriff | JustFOIA since 2026-09-07; GovQA for older requests | `pascosheriff.justfoia.com`, `pascocountysheriff.govqa.us` |
| Manatee Sheriff | GovQA (older domain) | `manateecountyfl.mycusthelp.com` |
| Largo, Clearwater, Oldsmar, Pinellas Park, Treasure Island, St. Pete Beach, Madeira Beach, Indian Rocks Beach | JustFOIA | `largofl`, `clearwaterfl`, `myoldsmar`, `pinellas-park`, `mytreasureisland`, `stpetebeachfl`, `madeirabeachfl`, `indian-rocks-beach` `.justfoia.com` |
| Manatee County | JustFOIA | `mymanatee.justfoia.com` |
| Hillsborough Clerk, Manatee Clerk | JustFOIA | `hillsclerk.justfoia.com`, `manateeclerk.justfoia.com` |
| HART (Hillsborough transit) | NextRequest | `gohart.nextrequest.com` |
| Hernando County | NextRequest | `hernandocountyfl.nextrequest.com` |
| Hillsborough County | ServiceNow customer portal | `hcfl.gov/AtYourService` → `hcflgov.service-now.com/cs` |
| Hillsborough, Pinellas, Pasco, Manatee school districts | a form on the district site; ScribOrder for student records | — |
| Tampa Bay Water | email, phone, mail | — |

No Tampa Bay agency other than HART runs NextRequest. 82 `{agency}.nextrequest.com` names
were tried (the agencies above, plus transit, ports, airports, housing, colleges and nearby
cities): an unknown name answers 302 to CivicPlus's "requester direction" page, a
client answers 204 at `/`. Hernando was found by search. It is the fourth county of the
Tampa–St. Petersburg metro, so it is counted here.

### GovQA — robots.txt turns every crawler away

Every GovQA host except one serves the same file:

```text
User-agent: PowerMapper
Allow: /

User-agent: *
Disallow: /
```

That is 11 hosts: Tampa, Safety Harbor, Gulfport, Pinellas County, the Pinellas Clerk, PCSO,
Pasco County (both names), the old Pasco Sheriff portal, and the two `mycusthelp.com` hosts
(St. Pete and Manatee Sheriff, the same file without the PowerMapper line). The product has
a public archive, `OpenRecordsSummary.aspx` → `RequestArchiveDetails.aspx?rid=…`. I fetched
Tampa's `SupportHome.aspx` once, in the same command as its robots.txt and before I had read
the file. That page links the archive. Nothing else on a disallowed host was fetched.

**Hillsborough Sheriff is the exception:** it serves no robots.txt (404). Its archive renders
**"No Records to display"** on first load, and the filter is a DevExpress postback. Two
`RequestArchiveDetails.aspx?rid=` guesses (1, 100000) went to `error.aspx?err_msg=Issue+Not+Found`,
which answers 200, entry 2's fault again. So it has an archive page and publishes nothing in
it.

Addresses carry an ASP.NET cookieless session, `/WEBAPP/_rs/(S(5b5kze25hsaj3hlyrccpvlcv))/…`.
The crumbs hold 12 distinct session paths across 6 hosts. Any strategy would have to cut the
`(S(…))` segment out to get one address per page.

### JustFOIA — a Cloudflare challenge on the one path robots.txt allows

All 12 JustFOIA hosts serve one byte-identical robots.txt:

```text
User-agent: *
Crawl-delay: 10
Disallow: /
Allow: /publicportal$
Allow: /publicportal/$
Allow: /publicportal/requests
```

`/publicportal` answers 200 with a 1.5 KB single-page-app shell. Its scripts, under
`/publicportal/dist/`, are disallowed, so their API was not read. `/publicportal/requests`,
the allowed path and the one Pasco Sheriff's own page describes as a searchable archive,
answered **403 with a Cloudflare managed challenge** ("Enable JavaScript and cookies to
continue") at all three hosts tried: Largo, Clearwater, Pasco Sheriff. Centinel does not
solve challenges, so whether any of the 12 publishes requests is unknown. The root of each
host redirects to staff sign-in (`auth.justfoia.com`).

### NextRequest — open, and the Tampa Bay clients publish almost nothing

The product is open to crawlers. robots.txt (the same 571 bytes on every host and on
`assets.nextrequest.com`) disallows only named SEO bots. The front end is a Vue app over a
JSON API that answers without a session:

| Address | Answer |
|---|---|
| `/client/requests?page_number=N` | `{total_count, requests:[…]}`: id (`26-122`), date, state, departments, point of contact, `request_text`, `requester_name` |
| `/client/requests/{id}` | one request: the text as HTML, departments, submit type, a `requester` object |
| `/client/requests/{id}/timeline?page_number=N` | the request's history; "Document(s) Released" entries list the released file names |
| `/client/request_documents?request_id={id}&page_number=1` | the request's **public** documents |
| `/client/documents?page_number=N` | every public document the portal holds |
| `/documents/{id}` | an HTML page for one document |
| `/documents/{id}/download` | **302** to `nextrequestdev.s3.amazonaws.com/{client}/{request}/{uuid}?…X-Amz-…`, a signed URL that expires |

The numeric document `id` decides the file. `/documents/{id}/download` is the stable address,
and the S3 location must never be kept, because it stops working when the signature expires.
`page_size` is refused above 50 (`422 {"page_size":["must be less than or equal to 50"]}`).
`sort_field=created_at&sort_order=desc` returns newest first.

What the two Tampa Bay clients publish:

| Portal | Public requests | Public documents |
|---|---|---|
| HART | **12**, 2023-09-27 to 2026-09-22, every one closed. Ids reach `26-122`, so about 4 of 2026's 122+ requests are published | **0**: `request_documents` is empty for all 12. Request 26-122's timeline shows ten files "released" to the requester, and none of them is public |
| Hernando County | **0** | **0** |

To show the shape when an agency does publish, Miami was measured with five requests: 51,417
public documents, 50 per page, newest uploaded 2026-11-20, oldest from 2018. A Tampa Bay
agency could turn publishing on at any time, and the API would then be the route.

### Requester names

- **NextRequest** has a `requester_name` field, and a `requester` object on each request. Both
  were null on all 12 HART requests and on Oakland's first page. But HART staff paste the
  emailed request into `request_text`, and request 26-122 carries the requester's name, title,
  email address, and the names and payroll numbers of the five employees whose records were
  requested. Requester identity can therefore appear in the text even where the field is
  empty.
- **GovQA** and **JustFOIA**: not seen. The archives were disallowed, challenged or empty.

Nothing is built here, so nothing indexes requesters. If a NextRequest strategy is ever
built, it should keep the text as published and leave `requester_name` alone.

### Against the three levers

None. Every finding is at **enumerate**, and for two of three vendors the block is the
vendor's own choice, not a fault. It is entry 7's shape again: a vendor that admits only
named bots (GovQA admits PowerMapper; Granicus admits Googlebot). JustFOIA adds a new
version: robots.txt allows the path and Cloudflare refuses it anyway.

### Cost

| What | Requests | Yield |
|---|---|---|
| HART, everything public | 1 list page + 12 × (request, timeline, documents) = **37**, about 40 s | 12 request records, about 3 KB of text, 0 documents |
| Hernando | 2 | nothing |
| The 11 GovQA hosts | — | needs an `ignore_robots` ruling per host, then an ASP.NET archive grid behind postbacks; the archive size is unmeasured |
| The 12 JustFOIA hosts | — | not reachable without solving a challenge |
| *Miami, for scale (not Tampa Bay)* | 1,029 list pages + 51,417 downloads | 51,417 documents |

### What would be built, and why it was not

The brief said to build a strategy for a platform whose **public released documents** recur
at two agencies. NextRequest recurs at two, and publishes documents at neither. Building one
for 12 short request texts is not worth it, so **no strategy was written**.

If it is wanted later, the measurements above settle its shape:

- keyed on the product (the `nextrequest.com` host, or the API's JSON);
- `client/documents?sort_field=created_at&sort_order=desc&page_number=N`, 50 per page, newest
  first, so a short walk keeps the newest;
- each document kept once, as `/documents/{id}/download`;
- each request kept as `/client/requests/{id}` (its text) and its timeline;
- bounded by the seed (`upload_start_date`/`upload_end_date`, which the API accepts) plus
  `--max-sitemaps`.

Its first real client would be outside Tampa Bay: Miami, Orange County Sheriff, Oviedo.

### Source-add commands

None to run. No portal here has anything collectable that an existing strategy can reach:
GovQA hosts need an operator ruling, JustFOIA can't be reached, and HART's 12 requests are an
SPA with no links for `links` to follow.

### Left on the table

- An `ignore_robots` ruling for the GovQA archives. Florida's records law makes the
  requests public records, but the vendor's file says no to every crawler. That is Ben's
  call, made per source with `investigate --ignore-robots` first (entry 7).
- Hillsborough County's ServiceNow portal. Its robots.txt is empty, but requests there are
  per account. Not explored.
- The school districts and Tampa Bay Water take requests by form or email. There is no
  portal to collect.

### Running log

- 2026-11-22 — 30 Tampa Bay agencies catalogued (25 portals) from the box's crumbs and agency pages.
  robots.txt read at 11 GovQA, 12 JustFOIA and 3 NextRequest hosts. Tampa's GovQA
  `SupportHome.aspx` fetched once before its robots.txt was read. JustFOIA's
  `/publicportal/requests` challenged at Largo, Clearwater and Pasco Sheriff. HCSO's GovQA
  archive empty. NextRequest's API mapped at HART and Hernando (12 requests, 0 documents)
  and at Miami for scale (51,417 documents; `/documents/{id}/download` → signed S3). No
  strategy built: no platform here publishes documents at two agencies.

## 10. CivicClerk — a public OData API behind a portal with nothing in it

*Seen 2026-11-22, with plain HTTP requests at one a second against Seminole
(`seminolefl`), St. Pete Beach (`stpetebeachfl`) and Tarpon Springs (`tarponspringsfl`), and
with the box's `log/*/source.toml` and crumbs. The store was read, not written.*

Seminole, St. Pete Beach and Tarpon Springs publish their meetings on CivicClerk (entry 9's
table, from crumbs). No strategy covered it: the box's three sources for those cities are
their CivicEngage sites, and each holds only the links.

### Where each city's tenant is

From the cities' own pages, in the box's crumbs:

| Source | Linked from its site | Tenant |
|---|---|---|
| seminole | `https://seminolefl.portal.civicclerk.com/` (12 pages) | `seminolefl` |
| tarpon-springs | `https://tarponspringsfl.portal.civicclerk.com/` (28 pages) | `tarponspringsfl` |
| st-pete-beach | `https://stpetebeachfl.civicclerk.com/Web/Player.aspx?id=414&…` (9 pages) | `stpetebeachfl` |

St. Pete Beach's site still links the product's older address; it answers a 301 to
`https://stpetebeachfl.portal.civicclerk.com`. Every tenant's portal is the same 2,808-byte
page: a title, *Public Portal • CivicClerk*, and a script. No meeting, no link.

### Where the meetings are

The portal's script names its API — `baseUrl:` `https://[TENANT].api.civicclerk.com/v1` —
and takes the tenant from the portal host's first label. The brief's guess was right, and
it held at all three tenants. No token, no cookie.

- **`/v1/Events`** is OData. `$orderby=startDateTime desc` and `$filter` work, form-encoded
  too (`%24filter=startDateTime+ge+2026-08-22`). Pages are **fifteen events whatever `$top`
  asks**: `$top=100` and `$top=1000` answered fifteen and a `nextLink` asking `$top=85` and
  `$top=985`. Each page names the next as `@odata.nextLink`, with a `$skiptoken` and the
  filter carried. `$count=true` answers 500.
- An event lists its **published files** — `{"fileId":747,"type":"Agenda",…}` — and names
  its agenda, `agendaId`. Types seen: `Agenda`, `Agenda Packet`, `Minutes`, `Notice`,
  `Other`. A scheduled event with no agenda yet has `agendaId` 0, or an agenda id and no
  files.
- **`/v1/Meetings/{agendaId}`** is the agenda: items nested as `childItems` (two levels at
  Tarpon Springs), each with `attachmentsList` — the documents attached to the item — and
  `reportsList`, the memo the clerk's template writes. An unpublished agenda (Seminole 168)
  answers 562 bytes and no items, so an event with no published file is not worth opening.

| Tenant | Events | Pages | Span | With a published file | Files |
|---|---|---|---|---|---|
| seminolefl | 101 | 7 | Apr 2024 – Jan 2027 | 92 | 171: 92 agendas, 79 minutes |
| stpetebeachfl | 2,322 | 155 | Jul 2008 – Dec 2027 | 2,155 | 4,615: 2,037 agendas, 602 packets, 1,920 minutes, 40 notices, 16 other |
| tarponspringsfl | 608 | 41 | Feb 2024 – Dec 2027 | 406 | 737: 404 agendas, 316 packets, 17 minutes |

The calendars run a year ahead: the newest pages are scheduled meetings with nothing
published.

### Which address holds a document

**Not the JSON's own links.** An attachment's `pdfVersionFullPath` and a report's
`pdfMediaFullPath` are `https://civicclerk.blob.core.windows.net/stream/{TENANT}/….pdf?…`,
signed and expiring about a day later (`se=` in the query). Unsigned, the same blob answers
404. An event's `publishedFiles[].url` (`stream/SEMINOLEFL/….pdf`) is the same blob path.
None of these is an address a document keeps.

The portal downloads through the API instead, by three routes in its script. Measured on
Seminole's agenda 747 and attachment 2490:

| Request | Answer |
|---|---|
| `Meetings/GetMeetingFileStream(fileId=747,plainText=false)` | 200 `application/pdf`, 188,560 bytes |
| `Meetings/GetMeetingFileStream(fileId=747)` | 404 |
| `Meetings/GetMeetingFileStream(plainText=false,fileId=747)` | 404 |
| `Meetings/GetMeetingFileStream(fileId=747,plainText=true)` | 200 `text/plain`, 3,740 bytes: the file's text |
| `Meetings/GetMeetingFileStream(fileId=999999,plainText=false)` | **200 `application/pdf`, 0 bytes** |
| `Meetings/GetAttachmentFile(fileId=2490)` | 200 `application/pdf`, 158,762 bytes, the attachment's `fileSize` |
| the same with `?foo=1`, or the path lowercased | the same bytes |
| `Meetings/GetAttachmentFile(fileId=999999)` | 404 |
| `Meetings/GetAttachmentFile(fileId=1525)`, a report's id | 404 |
| `Events/GetEventFileStream(fileId=747,fileType=1,plainText=false)` | 500, empty |

So a published file is `GetMeetingFileStream(fileId=N,plainText=false)`, both keys in that
order, and an item attachment is `GetAttachmentFile(fileId=N)` by the attachment's `id`. An
unknown published-file id answers an empty 200 — entry 2's shape, a success that holds
nothing — so the walk writes only ids the API listed.

**The same at the other two.** St. Pete Beach: packet 8152, 63,585,621 bytes; minutes 8208,
900,616; a notice 7804, 169,477; an `Other` file 8079, 5,629,807. Tarpon Springs: agenda
3802, 170,276; attachment 7842, 161,548. All `application/pdf`.

**An attachment comes back as a PDF whatever was uploaded.** St. Pete Beach 15668 and 15766
are Word documents (`contentType` `…wordprocessingml.document`) and answered PDFs of 72,953
and 73,100 bytes; 15640, a JPEG, answered a one-page PDF. Of 433 attachments in 31 meeting
records read, none was unpublished, a link, deleted, or without a PDF rendition.

**Reports have no stable address.** The portal shows a report from its signed URL and from
nothing else; no API route takes its id. So reports are not collected. Where a tenant
publishes a packet, the packet binds them (`includeInPacket: 1` on all 110 seen). Seminole
publishes no packets, so its memos are out of reach.

### Item attachments, by tenant

Meetings sampled every seventh across each tenant's years:

| Tenant | Sampled | With items | Attachments | Per meeting with items |
|---|---|---|---|---|
| seminolefl | 6 (2024–2026) | 6 | 64 | 10.7 |
| stpetebeachfl | 14 (2010–2026) | 8 | 98 | 12.3 |
| tarponspringsfl | 6 (2024–2026) | 6 | 148 | 24.7 |

St. Pete Beach's six meetings from 2010 to 2019 have no items: the agenda was uploaded
whole, and the event says `"publishedAgendaTimeStamp":"Agenda Posted on January 1, 1990
12:00 AM"`, a migration's placeholder. One tenant shows it, so it earns no lever: those
meetings cost a request each and give nothing, and they are the oldest, so they are the
last a walk reaches.

### robots.txt

Every portal and API host serves Cloudflare's managed template: `User-agent: *` /
`Allow: /`, with `Content-Signal: search=yes,ai-train=no,use=reference`, and named AI
training crawlers (GPTBot, ClaudeBot, CCBot, Google-Extended and two dozen more) disallowed
outright. No `Crawl-delay`. The blob host serves no robots.txt (404). Centinel is not a
named crawler and the rules allow it. The content signal grants search and withholds
training; `ai-input`, feeding a model at answer time, it neither grants nor withholds.

### Cost, and how a walk is bounded

Every page of events first, every published file kept from them; then one request per
agenda with a published file, newest first. At one request a second:

| Walk | Discover requests | Time | Published files | Attachments (estimate) |
|---|---|---|---|---|
| Seminole, portal | 7 + 92 = **99** | 1.7 min | 171 | about 980 |
| St. Pete Beach, portal | 155 + 2,155 = **2,310** | 38.5 min | 4,615 | about 10,200 (830 meetings since 2021 × 12.3) |
| Tarpon Springs, portal | 41 + 406 = **447** | 7.5 min | 737 | about 10,000 |
| The 90 days to Nov 22, a `$filter` seed | Seminole 1 + 8, St. Pete Beach 3 + 32, Tarpon Springs 3 + 34 | under a minute each | 15, 76, 61 | about 85, 390, 840 |

At the default 200-request budget a portal seed reads all of Seminole; Tarpon Springs'
pages and its newest 159 agendas; St. Pete Beach's 155 pages and its newest 45 agendas,
and says `truncated`. The St. Pete Beach packet above is 63.6 MB: collecting is the larger
cost wherever packets are published.

### Which box sources would switch

None switch: the three site sources stay on `sitemap`. Each city would gain a source
seeded with its portal and `strategy = "civicclerk"`. No box source was changed.

### Against the three levers

None. Enumeration again: the content is PDFs that read; the meetings are behind a script
that calls an API, and the API's own links expire.

### Left on the table

- Item reports (the clerk's memos), whose only address is signed for a day.
- `Meetings/{agendaId}` itself: each item's title, outline number, description and
  `minutesItemVotes`. Searchable text no PDF holds as data — but every answer carries
  freshly signed blob URLs, so its bytes change on every fetch, and it would be a new
  version every run.
- The plain-text rendition, `plainText=true`, which the extractor does not need.
- `Events/GetEventFileStream(…)`, the portal's route for event-level files; it answered
  500 for every id tried, and every file seen is reachable through `GetMeetingFileStream`.
- The meeting's name and category as the document's title (entries 2, 8 and 9).
- CivicWeb (Oldsmar, Madeira Beach, Largo), the last vendor in entry 7's table.

### Running log

- 2026-11-22 — robots.txt read on the portal, API, legacy and blob hosts. The portal's
  script read for the API base and download routes. Events walked to the end at all three
  tenants; file routes, keys, unknown ids and attachment renditions measured; meetings
  sampled across years. `civicclerk` written; unbuilt and untested here.

---

## 14. Meeting video — Granicus players and the cities' YouTube channels

*Seen 2026-11-22, with plain HTTP requests at one a second against seven Granicus clients
and YouTube, and `doctor`, `status` and the crumbs on the box (v0.36.0), read and not
written. Granicus was measured with `ignore_robots`, on Ben's ruling for these pages.*

Council and commission meetings are the largest public record Tampa Bay produces that the
corpus holds none of. The box has **no channel source**, **no audio blob** among its 110,366
addresses, and no transcription server: `doctor` reports the `transcription` gate blocked,
`[inference.transcribe]` unset, so the default (`whisper-large-v3-turbo` at
`http://127.0.0.1:8080/v1`) is asked and nothing answers. Its `yt-dlp` is 95 days old.

### Granicus: where a recording is, and where its words are

Seven clients publish video, and every one links its player the same way: not an `href`, an
`onclick`, `window.open('//{client}.granicus.com/MediaPlayer.php?view_id=V&clip_id=C',
'player', …)`. `MediaPlayer.php` answers 302 to `player/clip/C?view_id=V&redirect=true`, a
76–100 KB page holding

```html
<source type="application/x-mpegurl" src="https://archive-stream.granicus.com/OnDemand/_definst_/mp4:archive/clearwater/clearwater_<uuid>.mp4/playlist.m3u8">
<track kind="captions" srclang="en" label="On" src="/videos/5317/captions.vtt">
…
<a href="https://archive-video.granicus.com/clearwater/clearwater_<uuid>.mp4">Video Download</a>
```

**The recording is refused.** Every route to it ends at the same MP4: the player's
download link; `DownloadFile.php?view_id=V&clip_id=C` (302 to it); the publisher's
`ViewPublisherRSS.php?mode=podcast` enclosures, which are that `DownloadFile.php`; and
`ASX.php`, an ASX playlist naming `rtmp://…/clearwater_<uuid>.mp4`. St. Petersburg and
Hillsborough also link the MP4 straight off the publisher (2,440 and 1,274 links).
`archive-video.granicus.com` answered a `HEAD`, a `Range: bytes=0-4095` GET, and the same
GET with the player as `Referer` with **403 from CloudFront, "Request blocked"**; the HLS
playlist on `archive-stream.granicus.com` and `archive-video`'s own `robots.txt` answered
the same. Whether another client is let through was not tried: this crawler does not change
the name it gives. The `<uuid>` is in no other address, so even an allowed fetch costs the
player page first.

**The captions are not.** `/videos/{clip}/captions.vtt` is on the client's own host,
answers `text/vtt;charset=UTF-8`, and holds what a captioner typed: speaker turns
(`>>Mayor Cretekos: like to call to order…`, Clearwater 2014), `[ GAVEL ]`, Gulfport's
tracks opening `[Captioner present]`. Clearwater's City Council of November 5, 2026 is
5,336 cues, 302,028 bytes, from `00:02:10` to `02:59:56`.

| Query | Answer |
|---|---|
| `/videos/5317/captions.vtt` | the track, 302,028 bytes |
| `/videos/5317/captions.vtt?view_id=1` | the same bytes |
| `/videos/5338/captions.vtt` (a clip nobody captioned) | 200 `text/vtt`, 8 bytes: `WEBVTT` and two line breaks |
| `/videos/99999999/captions.vtt` | 500, a Slim framework error page |

So the clip id is the address. The player's searchable-transcript tab (`GUB-91023` in its
CSS) is not wired up on any page fetched; the captions file is the transcript.

Per client, ten clips sampled evenly across each publisher's clip ids (Clearwater also the
newest council meeting and a 2014 one):

| Client | Publisher | Recordings | Hours listed | Captioned | Bytes a captioned track |
|---|---|---|---|---|---|
| Clearwater | `clearwater.granicus.com`, view 50 | 2,374 | 3,956 | 4 of 10; council back to 2014, boards in the sample from 2021 | 18 K–302 K |
| Gulfport | `gulfport.granicus.com`, view 18 | 173 | 356 | 8 of 10 | 72 K–377 K |
| Dunedin | `dunedin.granicus.com`, view 6 | 1,498 | 2,963 | 8 of 10, back to clip 21 (June 21, 2007) | 10 K–966 K |
| Hillsborough County | `hillsboroughcounty.granicus.com`, view 1 | 1,274 | 2,410 | 9 of 10 | 22 K–425 K |
| St. Petersburg | `stpete.granicus.com`, view 14 | 2,440 | 6,472 | 0 of 5 | — |
| Safety Harbor | `safetyharbor.granicus.com`, view 6 | 619 | 1,059 | 0 of 5 | — |
| Treasure Island | `treasureislandfl.granicus.com`, view 1 | 816 | 1,041 | 0 of 5 | — |

"Hours listed" is the publisher's Duration column summed. Dunedin's newest clip and
Hillsborough's newest had empty tracks while the next ones were captioned, so a fresh clip
may be captioned later.

### What Centinel made of a track — and what it does now

Collected as a document, a track would have been stored and read by nothing:
`text/vtt` had no row in the content-kind table, `WEBVTT` was no magic anything knew, and
`.vtt` no extension, so it classified `other`, and `other` has no reader. The YouTube
caption reader parses `json3` only.

Built, in this entry's pull request:

- **`webvtt` is a content kind**, with a row (`vtt`, `text/vtt`) and a magic (`WEBVTT`).
  Not `captions`: a server calls the two `text/vtt` and `application/json`, they are named
  `.vtt` and `.json`, each has its own parser, and the reader that spoke is the record's
  account of who wrote the words. One kind would have recorded a publisher's captioner as
  YouTube's speech recognition.
- **A `webvtt` reader** beside the `json3` one, through the same extractor and the same
  timestamped markdown a Whisper transcript gets. An eight-byte track is unextractable with
  its reason. Pipeline version 5.
- **`granicus` keeps one track per recorded clip**, read off the publisher without a
  request, after the viewers. Uncaptioned tracks are kept too: which clips have captions
  cannot be told from the publisher, and a track is one collect request, once, where a
  probe would spend the discovery budget on every run. Transcription is not involved.

### Against the three levers

**Content kind** and **reader list**, both: the document was one address away and plainly
named, and the pipeline would have fetched it, called it `other`, and read nothing — the
silent shape entry 4 found for `.csv`. Not enumerate: the address set is on the publisher.

### YouTube: each government's channel

From the box's crumbs (the channels each city's own pages link, with the count of links),
and for Pasco, Manatee, Hernando and Clearwater, the county or city home page. Each channel
page was fetched once to read its canonical id and handle, and its `/streams` tab (else
`/videos`) for the newest titles.

| Government | Channel | Id | Newest, 2026-11-22 |
|---|---|---|---|
| Hillsborough County (meetings) | `@HillsboroughCountyMeetings` | `UCvdfJe4Q4u9YudwH89yJkDQ` | BOCC organizational meeting, 11.17.26 |
| Hillsborough County | `@hillsboroughfl` | `UC-ltrT7T2PDRdEEoU5NtOEg` | events and storm updates, not meetings |
| Pinellas County | `@PinellasGov` | `UCXRyq4zJBFhs8zpTiIGQL5Q` | streams end 2024; meetings are on Legistar |
| Pasco County | `@PascoCountyGovernment` | `UCvlOC1VxmqM8_CiIYqe8iyA` | BOCC 12.8.26, Planning Commission 11.19.26 |
| Manatee County | `@ManateeCountyFla` | `UCNb-baujcEbYoPffIc0Ch3g` | no `/streams` tab; videos are features |
| Hernando County | `@HernandoCountyGovernment` | `UCvW76oulWUK2sZpCFk5BB3A` | town halls |
| Tampa | `@CityofTampa` | `UCx4-4RHo_bhTMQJNh6Du0AA` | State of the City; council is CTTV (entry 3) |
| St. Petersburg | `@StPetersburgFL` | `UCAkNPf00JA9Ri0iKw7xFjPA` | "St. Pete TV Live Stream" |
| Clearwater | `@MyClearwater` | `UCKr9me5QJuC173tnPh_4k9g` | City Council 11/19/2026 |
| Largo | `@largotv1` | `UCp5f-PJDTpbHifaOTaotL2g` | streams stop at Sept. 2021 |
| Dunedin | `@CityofDunedinFL` | `UCHh0XD1oISXtkFGqExIoEMA` | City Commission 11-19-26 |
| Oldsmar | `@CityOfOldsmar` | `UCAzoS5eveYcUaWisO0ffQDA` | Council 11/17/26 |
| Safety Harbor | `@SafetyHarborFL` | `UC7MKhUqK-awjn_oZ23cTm2w` | City Commission 11-17-26 |
| Tarpon Springs | `@TarponSpringsTV` | `UCT6_SxI7RxFrdJLoHi-02ag` | Board of Commissioners 11/17/26 |
| Temple Terrace | `@cityoftempleterrace` | `UC4z9CgD1RQNAwFPMKbTKupw` | City Council 11/17/26 |
| Plant City | `@cityofplantcitygovernment8169` | `UC1gqU0211GQktDTpfUBpmNQ` | untitled streams |
| Pinellas Park | `@CityofPinellasPark` | `UCTjLCSCpW0bhlK7ZhLV7MVg` | City Council 10/20/26 |
| Treasure Island | `@cityoftreasureisland` | `UCU5ocMcwbaVkK5RgaKIZK_w` | Commission 11/17/26 |
| Madeira Beach | `@cityofmadeirabeach632` | `UCbcmYPPO9KWTxD2k2XUv5Iw` | Planning Commission 11/2/26 |
| South Pasadena | `@CityofSouthPasadenaFL` | `UCX_4EaDefLDHi9ppxq1UWFQ` | Regular Commission 11/10/26 (in `/videos`) |
| Belleair Beach | `@cityofbelleairbeachofficia4256` | `UCBTiCuq7bdOfOjqAnHY0zbA` | City Council 11-2-2026 |
| Redington Shores | `@townofredingtonshores` | `UC0uVrMAzJl66mAEQ3Bg8IdQ` | 2 streams, newest 2024 |
| Hillsborough TPO | `@hillsboroughtpo` | `UCsojHyZb_mkYIU3o32Tbg4w` | committee meetings, Nov. 2026 |
| Tampa Bay RPC | `@TampaBayRPC` | `UCmHpFM3taiu-hIjwdjBqPHA` | one stream |

Not governments' meeting channels, so not listed for collection: `@MyClearwaterFL` is
"Visit Clearwater Florida", the tourism channel Clearwater's pages link most; St. Pete
Beach's linked channel is its recreation department's. Gulfport's pages link no channel.

The three Granicus clients that caption nothing — St. Petersburg, Safety Harbor and
Treasure Island — all stream their meetings to YouTube, where a channel source gets
YouTube's auto-captions and, for the ~7% without, audio for `transcribe`.

### Cost

| | Requests | Download | Text |
|---|---|---|---|
| This measurement | 158 (92 Granicus GETs and a `HEAD`, 55 YouTube, 10 county and city sites) | 147 MB, most of it seven publisher pages (Dunedin's is 30 MB) | — |
| Discover, per Granicus source | 1 publisher page, plus the agenda manager's | 0.3–30 MB | — |
| Collect the four captioning clients' tracks | 5,319, about 1.5 hours at one a second | about 0.8 GB, estimated from the samples | about 6,400 hours of meetings already transcribed |
| Collect the three that caption nothing | 3,875 eight-byte tracks, about an hour, once | 31 KB | none |
| Transcription | 0 hours for Granicus | | |

A YouTube channel is a `yt-dlp` listing plus metadata, captions and, by default, audio for
uncaptioned videos: about 63 MB of audio each, and its hours go to the transcription server.

### What transcription needs on the box

A server for `[inference.transcribe]`. Nothing is configured: the section is absent, so
`doctor` asks the default, `whisper-large-v3-turbo` at `http://127.0.0.1:8080/v1`, and
gets `Connection refused`. The box is a 3.3 GB VM without AVX2 (its own config says so);
whisper-large there would be far slower than realtime even if it fit. The section wants a
`base_url` on a machine with a GPU, or a hosted OpenAI-compatible endpoint with its key
named in `api_key_env` — which sends meeting audio off the box, the choice `CONTEXT.md`
says the config file is the one place to make. Run the server with VAD. `ffmpeg` is
installed. `yt-dlp` (2026.08.19) wants updating before a channel source runs. Granicus
captions need none of this.

### Left on the table

- **A track that fills in later.** A clip whose captions were empty when collected is not
  fetched again; Dunedin's and Hillsborough's newest were empty while the clips before were
  not. Collect re-fetches nothing observed, so it wants `--refresh` on the newest clips, or
  a rule.
- **The meeting's name as the track's title.** The publisher row names it ("City Council
  on 2026-11-05 6:00 PM"); a track has no title, so its chunks sit under no heading. Entry
  2's lever 6 again.
- **Granicus clients beyond these seven.** Slugs are not guessable (research §8.1); each
  needs a crumb or a city's link.
- **The recording**, if Granicus's video host ever answers; the player page is the only
  place its address appears.
- Channel sources for the governments above; none was added.

### Running log

- 2026-11-22 21:00 — box read: no channel source, no audio, transcription gate blocked
  (`[inference.transcribe]` unset, nothing on `127.0.0.1:8080`), `yt-dlp` 95 days old.
- 2026-11-22 21:15 — Clearwater player measured: MP4 and HLS 403 from CloudFront;
  `/videos/{clip}/captions.vtt` serves WebVTT; empty track is 8 bytes, missing clip 500.
- 2026-11-22 21:20 — six more publishers: same player link at all seven; captions at
  Gulfport, Dunedin, Hillsborough County; none at St. Pete, Safety Harbor, Treasure Island.
  `DownloadFile.php`, podcast enclosures and `ASX.php` all end at the blocked MP4.
- 2026-11-22 21:25 — ten clips sampled per captioning client; `.vtt` found to classify as
  `other`. `webvtt` kind and reader written, `granicus` keeps one track per clip. Unbuilt
  here; CI is the first build.
- 2026-11-22 21:30 — 26 YouTube channels resolved (28 addresses) from the crumbs and four
  home pages; `/streams` sampled.

---

## 19. Budgets, ACFRs and codes of ordinances — what governments publish about their money and their law

*Seen 2026-11-22, with the box's `log/*/` and `crumbs/*.jsonl` read (not written), one
`centinel search` on the box, and plain HTTP requests at one a second or the host's
`Crawl-delay` from a laptop and, for the code publishers, once from the box.*

Two things every government publishes about itself and the corpus may lack: its **adopted
budget and annual comprehensive financial report (ACFR)**, and its **code of ordinances**.

### Part A — coverage on the box

Counted from every observed address whose name says budget, ACFR, CAFR or comprehensive
annual financial report, and is a PDF or spreadsheet; at these governments every one was
derived. "No source" means no box source covers the government's site at all.

| Government | Adopted budgets | ACFRs | Verdict |
|---|---|---|---|
| Tampa | budget books FY2007–FY2017; FY2027 recommended package. FY2018–FY2026 adopted books are only on OpenGov Stories | FY2016–FY2021, FY2024, FY2025 (FY2022, FY2023 not seen) | partly |
| Hillsborough County | FY2014–FY2027 adopted, recommended, CIP, in-brief: 86 PDFs on `assets.contentstack.io` | **none.** The Clerk publishes FY2019–FY2025 at `hillsclerk.com`; the box holds the page and none of the files | budgets covered, ACFRs missing |
| Pinellas County | no source. FY2027 adopted as a PDF on `pinellas.gov`; FY2025 adopted and the capital plans only on Questica OpenBook | the Clerk, `mypinellasclerk.gov`, answers a Cloudflare challenge | missing |
| St. Petersburg | FY2022–FY2025 adopted, FY2026 30-day adopted | 2022–2025 | covered |
| Clearwater | FY2025–26 operating and capital; budget brief; CRA budgets | 2025 | partly; older years not measured |
| Largo | FY2018 only. FY2025–FY2027 adopted, proposed, tentative and CIPs are on Questica OpenBook | FY2019–FY2025 linked from `largo.com/document_center`, none collected | missing |
| Pasco County | no source. FY2022–FY2026 adopted PDFs on `pascocountyfl.gov`; FY2026–2030 CIP only on OpenGov Stories | not on the Clerk's Financial Reports page; not found | missing |
| Manatee County | no source. FY2016–FY2026 adopted books named in `mymanatee.org`'s sitemap | the Clerk, `manateeclerk.com/departments/finance/financial-reporting/`; not measured further | missing |
| Hillsborough schools | no source; an Apptegy site | Apptegy | missing |
| Pinellas schools | no source; Finalsite, and robots.txt disallows `/fs/`, where Finalsite keeps files | — | missing |
| Pasco schools | no source; `pascoschools.org` timed out | — | not measured |
| Manatee schools | no source; Apptegy. ACFR 2023–24 and 2024–25 are in the folder page's script payload, on `files-backend.assets.thrillshare.com` | Apptegy | missing |

One search proves the counts are text and not titles: `annual comprehensive financial report
statement of net position`, `--source tampa`, answered five passages from the FY2025 ACFR's
statements of net position and fiduciary net position (BM25, 22 s).

### Why each gap is a gap

**A budget platform the crawl cannot see.** The crumbs name four: ClearGov at Clearwater,
Dunedin, Gulfport and Plant City; Questica OpenBook at Largo (433 links); OpenGov Stories at
Tampa (35); OpenGov Transparency at St. Pete. Each is a script on another host, so it is a
crumb and never a page.

- **ClearGov** is not a gap. All four ClearGov cities post the same adopted budget as a PDF on
  their own site, and the box holds it: Dunedin FY2025–FY2027, Gulfport FY2026, Plant City
  FY2020–FY2023, Clearwater FY2025–26. The book itself is an AngularJS shell over a 2.4 MB
  panel API (`/api/budgetBook/{id}/panels/all`), and the documents product (`*-cleardoc`) a
  second script app. Nothing to build.
- **Questica OpenBook** is a gap at two governments, and is built: `openbook` (below).
- **OpenGov Stories** is a gap at two governments, and is built: `opengov-stories` (below).
  Tampa posts no adopted budget PDF after FY2017; its budget pages link the books. Pasco posts
  its adopted budgets as PDFs, and its FY2026–2030 capital plan only as a book.

**A page that lists more documents than a page may enclose.** The enclosure scan keeps 25
documents a page (`enclosure::MAX_PER_PAGE`), because "a page carrying more than this is a
listing, and a listing's documents belong to `enumerate`". `links` does not follow a link to
a document, because "a PDF linked from a page is found again by the enclosure scan". Each
defers to the other, so a listing's 26th document is collected by neither. Three sightings,
all of them a government's own finance documents:

| Page | Documents linked | What the box holds | Lost |
|---|---|---|---|
| `largo.com/document_center/index.php` (walked by `links`) | 165 PDFs | 10 | ACFRs FY2019–FY2025, at positions 112–120 |
| `pascocountyfl.gov/…/office_of_management_and_budget/index.php` | 38 documents | no source yet | adopted FY2022–FY2024, at positions 27, 28, 33 |
| `hillsclerk.com/records-and-reports/financial-reports-county` | 99 links | the page, no file | ACFRs FY2019–FY2025, and the links name no extension |

The lever is a framework decision, not a strategy, and it changes what every source collects,
so it is proposed here and not built: **a page's documents past the cap are addresses**, and
go back to the pass's frontier, bounded by the source's address ceiling, rather than being
counted and dropped.

**A document link with no extension.** Hillsborough's Clerk runs Liferay, and links each file
as `/documents/d/guest/hillsborough-fy24_acfr_final-pdf?download=true`. The enclosure scan
decides a document by its extension, so none is a candidate even below the cap. One sighting.

**No source at all.** Pinellas, Pasco and Manatee counties and all four school districts. For
the three counties, an existing strategy collects most of it (commands below).

### Questica OpenBook (`openbook`)

`pinellascounty.openbook.questica.com` and `largo.openbook.questica.com` serve one shell,
titled `OpenBook`, that draws everything from a JSON API on the same host. `robots.txt`
redirects to `/error` (no rules). A book is at `#/budget-book/{key}`; the same route as a path
answers the error page.

| Request | Answer |
|---|---|
| `api/home/cards` | the front door's cards. `cardType: "BudgetBook"` cards carry the book's **version** id as `id` (Largo 11 of 13 cards, Pinellas 3 of 18) |
| `api/digitalbudgetbookviewer/versionIds/FY27AdoptedBudget` | `"395fc3b1-e3fe-4419-aa94-1008fbee725a"`, the version for a custom name |
| `…/versionIds/{a card's id}` | the nil GUID: the route takes names, not ids |
| `api/digitalbudgetbookviewer/versions/{version id}` | the book: `name`, contents, `showPDFLink`, `publishedPdfHref` |
| `…/versions/FY27AdoptedBudget` | 400, `"The specified budget book version could not be found."` |
| `publishedPdfHref` | `opendatastorageprodus.blob.core.windows.net/{client}-pdf-{version}/{name}.pdf`: `application/pdf`, ranges honoured; that host's `robots.txt` answers 400 |

All fifteen published PDFs, measured: Largo's eleven, 12.7 MB to 198 MB (the four CIPs are
184–198 MB); Pinellas's four, 27 MB (FY2027 adopted), 60 MB (FY2025 adopted), 319 MB and 366 MB
(the FY2026–31 and FY2025–30 six-year capital plans). Total 1.81 GB.

**Pinellas hides half its books.** The FY2026 and FY2027 adopted books and both newer capital
plans have their cards turned off and are linked only from `pinellas.gov` by custom name. Four
of Pinellas's eight books publish no PDF (`showPDFLink: false`), FY2026 adopted among them.
One page of that book, `pages/{id}`, answered `publishedMarkup: " "`: its text is somewhere
this did not find.

**Why the book seed is the API address.** A seed of `…/#/budget-book/FY26-31_CIP` arrives at
the strategy as the front door: reqwest records a response's URL from tower-http's
`RequestUri`, an `http::Uri`, which has no fragment. So a book is seeded by its viewer address,
`…/api/digitalbudgetbookviewer/versionIds/FY26-31_CIP`, and that answer is the seed, so the
walk does not ask for it again.

**What `openbook` does.** Front door: the cards, then each book's version, keeping each
`publishedPdfHref` where `showPDFLink` is true, and naming in a warning each book that
publishes none. A book seed: the version, then the same. Every address is a document.

**Cost.** Largo's front door: 1 + 11 + 1 (the blob host's `robots.txt`) = 13 discover
requests; collect is 11 PDFs, 1.04 GB. Pinellas's front door keeps one PDF, the FY2025–30 CIP,
and the default 256 MiB body ceiling refuses it; its FY2025 adopted book (60 MB) is a book seed.

### OpenGov Stories (`opengov-stories`)

Entry 1's finding holds three months on, at both clients. A book's contents page,
`stories.opengov.com/{client}/{uuid}/published/{slug}?currentPageId=TOC`, still says "Preview
link expired" and still names every page in `data-public-url`:

| Book | Entries |
|---|---|
| Tampa FY2027 Online Budget (`mHPI_6Pa9`, entry 1's) | 75 |
| Tampa FY2026 Operating & Capital Budget (`4rwPtXRZO`) | 74 |
| Tampa FY2025 Online Budget (`qrLf8u3HE`) | 74 |
| Tampa FY2024 Online Budget (`ib-4Hif6A`) | 74 |
| Pasco Budget Book FY 2026 (`eBgL6jMBk`) | 105 |
| Pasco Capital Budget Book FY 2026 (`4EufQHtfa`) | 16 |

A public page is server-rendered text: Pasco's "Introduction & Overview", 20 KB, 2,660
characters of narrative, no `data-public-url`, a few neighbour links. The contents page's only
other link is `https://opengov.com`. `robots.txt` is a 404. Tampa's FY2023 link
(`/tampa/published/sJ8L0XY6r`) is a public page, not a contents page, so it is not a seed.

**What `opengov-stories` does.** Recognises a page whose anchors carry class `toc-story-link`
and a `data-public-url`, and keeps each such address on the same host, in order. No request:
the seed is the walk. The pages are pages, so acquisition still scans them for documents.

### Part B — codes of ordinances

**Where they are.** The crumbs name the publishers. Municode (`library.municode.com`) is
linked from 30 of 43 box sources: Belleair Beach 880 links, Indian Rocks Beach 1,316, Plant
City 1,044, Belleair Bluffs 1,030, St. Pete 810 (and 119 to `www.municode.com`), Treasure
Island 551, Gulfport 596, Largo 506, and Tampa, Hillsborough and Pinellas County among them.
eCode360 (General Code) at South Pasadena, 490 links. American Legal (`codelibrary.amlegal.com`):
no box source links it.

**Measured, and refused.** Every publisher's host answers a Cloudflare managed challenge to
Centinel's User-Agent:

| Address | Answer |
|---|---|
| `library.municode.com/fl/tampa/codes/code_of_ordinances` | 403, `cf-mitigated: challenge`, "Just a moment..." |
| `api.municode.com/Clients/name?clientName=tampa&stateAbbr=fl` | 403, challenge |
| `api.municode.com/`, `library.municode.com/sitemap.xml`, `www.municode.com/`, `municode.com/` | 403, challenge |
| the same two from the box's address | 403, challenge |
| the same two from Python's `urllib` instead of curl | 403, challenge |
| `codelibrary.amlegal.com/codes/pompanobeach/latest/overview`, `…/api/client-regions/` | 403, challenge |
| `ecode360.com/SO1390` | 403, challenge |

Yet each `robots.txt` answers 200 and admits every crawler not named: `User-agent: *`,
`Allow: /`, under Cloudflare's managed block, which disallows named AI crawlers by name and
sets `Content-Signal: search=yes,ai-train=no,use=reference`. American Legal adds
`Crawl-delay: 5` for a list of SEO bots; eCode360 disallows `/print`, `/output`,
`/documents`, `/search` and others, and bingbot outright. So the rules let Centinel in and the
edge does not.

**Not built.** A strategy that cannot fetch its first answer would be a confident
half-answer, and getting past a bot challenge is evasion, not collection. What would unblock
it, for Ben to choose: ask the publishers to allow Centinel's User-Agent (all three declare
`search=yes`), or a city's own copy of its code. Not measured here: the JSON routes behind
the challenge, which keys decide a section, and whether full-text or PDF downloads exist.

### Against the three levers

None for the strategies built: enumeration, both of them, of PDFs and HTML that read. The
listing-cap gap is the **enclosure** lever: the documents are one address away, and the cap
drops them.

### Ready to run

Needs this pull request's build for the two new strategies; the rest run on v0.37.0. A source
id is a suggestion.

```sh
# Questica OpenBook
centinel source add largo-openbook --site https://largo.openbook.questica.com/ --strategy openbook
centinel source add pinellas-budget-fy2025 --site https://pinellascounty.openbook.questica.com/api/digitalbudgetbookviewer/versionIds/FY25Adopted --strategy openbook

# OpenGov Stories
centinel source add tampa-budget-fy2027 --site 'https://stories.opengov.com/tampa/79cac2e9-83c9-4198-b664-6903cc9d9338/published/mHPI_6Pa9?currentPageId=TOC' --strategy opengov-stories
centinel source add tampa-budget-fy2026 --site 'https://stories.opengov.com/tampa/f9ac8a22-1024-454b-bb87-f5f980e98715/published/4rwPtXRZO?currentPageId=TOC' --strategy opengov-stories
centinel source add tampa-budget-fy2025 --site 'https://stories.opengov.com/tampa/7e25e05b-cbac-4b7f-ab7f-3497a3ac5138/published/qrLf8u3HE?currentPageId=TOC' --strategy opengov-stories
centinel source add tampa-budget-fy2024 --site 'https://stories.opengov.com/tampa/ff0baa7f-cfd2-4e5f-9094-189c84b0ca98/published/ib-4Hif6A?currentPageId=TOC' --strategy opengov-stories
centinel source add pasco-cip-fy2026 --site 'https://stories.opengov.com/pascocountyfl/12ec6ff1-6593-4332-9f8f-144328e0ead4/published/4EufQHtfa?currentPageId=TOC' --strategy opengov-stories

# Existing strategies
centinel source add pinellas --site https://pinellas.gov/
centinel source add pasco --site https://www.pascocountyfl.gov/
centinel source add manatee --site https://www.mymanatee.org/ --match financial-management-department
```

- `pinellas` is a WordPress sitemap of 11,279 addresses; the FY2027 adopted book is enclosed by
  `adopted-fy27-annual-operating-and-capital-budget-print/`.
- `pasco` has no `robots.txt` and a 500-address sitemap that names the budget office's page;
  FY2026 adopted is enclosed, and FY2022–FY2024 fall past the 25-document cap until it is
  decided.
- `manatee`'s sitemap is 23,167 addresses, 269 under the finance department, which names the
  eleven adopted books directly; `--match` keeps the walk to them.
- Pinellas's FY2027 adopted book is on `pinellas.gov` (21.8 MB) and on OpenBook (26.9 MB, a
  different file); only the first is in these commands.

### Left on the table

- The listing cap (above): Largo's and Hillsborough's ACFRs, Pasco's older adopted budgets.
- Extensionless document links (Liferay `/documents/d/guest/…`).
- OpenBook books without a PDF: Pinellas FY2026 adopted, FY2027–32 CIP, FY2025 adopted "V1",
  FY2025–30 proposed CIP.
- The two Pinellas capital plans over the 256 MiB ceiling, a second sighting of *a real report
  bigger than the ceiling*.
- Apptegy, at two school districts: files on `files-backend.assets.thrillshare.com`, named only
  in each folder page's script payload (Manatee's ACFR folder page is 7.3 MB). The documents
  module's enumeration is not measured.
- The Pinellas Clerk (challenged), the Pasco and Manatee Clerks' ACFRs, Pasco schools.
- OpenGov Stories' charts: the figures are iframes from `{client}.opengov.com/transparency`.
- Codes of ordinances, all of them, until a publisher lets Centinel in.

### Running log

- 2026-11-22 — box read: 43 sources in `log/`, 139,062 observations. Budget and ACFR
  documents counted per source; Pinellas, Pasco, Manatee and the school districts have no
  source. One search confirms Tampa's ACFR text is indexed.
- 2026-11-22 — crumbs read: Municode at 30 sources, eCode360 at one, American Legal at none;
  ClearGov at four, Questica at Largo, OpenGov at Tampa and St. Pete.
- 2026-11-22 — Municode, American Legal and eCode360: every host challenges, from two
  addresses and two HTTP stacks; robots.txt allows. Part B stops at measurement.
- 2026-11-22 — ClearGov: AngularJS shell and panel API; every ClearGov city posts the PDF too. No gap.
- 2026-11-22 — Tampa's budget pages link only OpenGov books for FY2024–FY2027; the FY2026 contents
  page names 74 pages by `data-public-url`. Entry 1 holds.
- 2026-11-22 — Pinellas: WordPress sitemap; FY2027 adopted as a PDF; FY2025 and FY2026 on
  Questica. Questica's viewer API measured: cards → version → `publishedPdfHref`.
- 2026-11-22 — survey of Pasco, Manatee, both clerks and the school districts started.
- 2026-11-22 — Pasco's budget office links OpenGov Stories: second sighting. `opengov-stories`
  and `openbook` written.
- 2026-11-22 — every Largo and Pinellas book resolved and its PDF sized: 15 PDFs, 1.81 GB; two over
  the ceiling. A seed's fragment never reaches a strategy, so a book seed is its API address.
- 2026-11-22 — Hillsborough's ACFRs are on the Clerk's page as extensionless Liferay links; Largo's
  are at positions 112–120 of 165 on a page the box walked; Pasco's older adopted budgets past
  position 25. The listing cap, three sightings; proposed, not built.
- 2026-11-22 — Manatee: eleven adopted books in the sitemap. Apptegy at two school districts.

---

## 13. Special districts and authorities — the governments that are not a city or a county

*Seen 2026-11-22, with plain GETs at one request a second per host from a workstation, and
`centinel investigate` (v0.36.0) and `crumbs` on the box. The store was read, not written.*

Tampa Bay's special districts — community development districts (CDDs), community
redevelopment agencies (CRAs), and the authorities that run the port, the airports, transit,
water and the expressway — publish their own meetings, and the corpus held almost none of
it. Of the bodies in scope, two are box sources (`tbrpc`, and Plan Hillsborough as
`hillsborough-transportation`); three more reach the corpus through a city's or county's
agenda source (Tampa's CRA in OnBase, Clearwater's CRA board and PIE's governing board in
Legistar). The corpus links to **no CDD host at all**: of 4,460 crumb hosts, none is a
district's site.

### Finding the districts

The state's list is the Department of Commerce's official list of special districts
(`specialdistrictreports.floridajobs.org/OfficialList/CustomList`). It answers 403 with an
Azure WAF JavaScript challenge, robots.txt included, so it was not read. The Auditor
General's e-file index (`flauditor.gov/pages/special districts_efiles.html`) lists 1,689
districts and no county; the FRS's Appendix K, which has the county, never answered (HTTP/2
`INTERNAL_ERROR`, then a hang over HTTP/1.1). So the districts were found by name — legal
notices, management firms' agendas, search — and **24 of Hillsborough's roughly hundred CDDs
were sampled, across every management firm found, and two of Pinellas's three** (Entrada was
not found). This entry is a sample of the field, not a census of it.

Water Street Tampa's district is not a CDD: it is the **Water Street Tampa Improvement
District**, created by special act (ch. 2017-206, Laws of Florida), at `wstid.net`.

### The catalogue

`robots` is what the host's robots.txt says for this crawler. `TLS` is the root the served
chain ends at, when centinel's client refuses it.

**Districts (CDDs and the improvement district)**

| Body | Manager | Site | Platform | robots | TLS | Collected by |
|---|---|---|---|---|---|---|
| Water Street Tampa ID | — | `wstid.net` | `_assets` template; sitemap.xml by convention, 8 pages | `Disallow: /_assets/documents/` — **every document** | ok | nothing: pages only, documents need a ruling |
| South Fork III CDD | Inframark | `southfork3cdd.org` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| MTERC CDD | Rizzetta | `mterccdd.org` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Riverbend West CDD | — | `riverbendwestcdd.com` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Reserve at Pradera CDD | — | `reserveatpraderacdd.org` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Cypress Creek CDD | — | `cypresscreekcdd.org` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Waterset South CDD | Rizzetta | `watersetsouthcdd.org` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Highlands CDD | — | `highlandscdd.com` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Cordoba Estates CDD | — | `cordobaestatescdd.com` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Shell Point CDD | — | `shellpointcdd.com` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Westchase CDD | — | `westchasecdd.us` | Campus Suite | 404 | **Root YR** | blocked (TLS) |
| Panther Trace II CDD | — | `panthertracecdd2.net` | Campus Suite, behind Cloudflare | Cloudflare's comment header, no rules | ok | `campus-suite` |
| Highland Trails CDD | — | `highlandtrailscdd.org` | Campus Suite, behind Cloudflare | Cloudflare's comment header, no rules | ok | `campus-suite` |
| Heritage Harbor CDD | — | `heritageharborcdd.org` | Campus Suite, behind Cloudflare | Cloudflare's comment header, no rules | ok | `campus-suite` |
| K-Bar Ranch II CDD | — | `kbarranchiicdd.org` | Campus Suite, behind Cloudflare | `Allow: /`, content signals, AI crawlers named and disallowed | ok | `campus-suite` |
| Wynnmere East CDD | Rizzetta (was PFM) | `wynnmereeastcdd.com` | WordPress, `wp-sitemap.xml` | wp-admin only | ok | `sitemap`; `/documents` lists 75, enclosure keeps 25 |
| Newport Tampa Bay CDD | PFM | `newporttampabaycdd.com` (http) | WordPress, sitemap | wp-admin only | — | `sitemap` |
| Villages of Westport CDD | Vesta (proposed) | `villagesofwestportcdd.com` | WordPress, `wp-sitemap.xml` | wp-admin, wpforms | ok | `sitemap` |
| Ballentrae Hillsborough CDD | GMS | `ballentraehillsboroughcdd.org` | CommunityXS (Next.js) | `Disallow: /documents/*/download` — **every document** | ok | pages only; 494 documents disallowed |
| Park Creek CDD | GMS-Tampa | `parkcreekcdd.org` | CommunityXS | the same | ok | pages only |
| Waterleaf CDD | GMS-Tampa | `waterleafcdd.org` | CommunityXS | the same | ok | pages only |
| Cory Lakes CDD | GMS | `corylakescdd.net` | `_assets` template (`documents.php`) | `Disallow: /_assets/documents/` — **every document** | ok | pages only |
| Tampa Palms OST CDD | — | `tampapalmscdd.net` (http) | hand-built HTML | allows | — | `links` |
| S. Bay CDD | — | `sbaycdd.org` | — | — | — | did not answer, http or https |
| Clearwater Cay CDD (Pinellas) | — | `clearwatercaycdd.com` | WordPress, one post per meeting | 404 | ok | `sitemap` (441 by convention) |
| Eastlake Oaks CDD (Pinellas) | — | `eastlakeoakscdd.com` | hand-built HTML, a page per year | 404 | ok | `links` |

**CRAs.** A CRA's board is its city commission or county board, so its meetings ride on
that government's agenda platform (entry 7), not on a site of its own.

| CRA | Where its meetings are | Collected by |
|---|---|---|
| Tampa | OnBase: "CRA Regular", "CRA Special" (7 of 76 meetings, May–July 2026) | `tampa-agendas` |
| Clearwater | Legistar body "Community Redevelopment Agency"; the CRA's own site `myclearwatercra.com` (Granicus OpenCities, 11 pages) holds no agendas | `clearwater-legistar`; the site is not a source |
| St. Petersburg | inside the council's agenda (convenes as the CRA mid-meeting); files on `cms5.revize.com/revize/stpete/`, whose robots.txt admits only `*.pdf`, `*.doc(x)`, `*.ppt(x)`; 739 crumb links | **nothing**: off-host from `stpete.org` |
| Lealman (Pinellas County) | the BCC | `pinellas-legistar` (not checked by meeting) |
| Largo, Dunedin, Tarpon Springs, Oldsmar, Pinellas Park, Gulfport, Plant City, Temple Terrace | the city commission's platform (entry 7: CivicWeb, Granicus, CivicClerk, Document Center) | as entry 7 left them; not rechecked here |

**Authorities**

| Body | Site | Meetings published on | robots | Collected by |
|---|---|---|---|---|
| Port Tampa Bay | `porttb.com` (WordPress, sitemap) | **BoardBook Premier**, `meetings.boardbook.org/Public/Organization/1448`: 22 meetings, each `Public/Agenda/1448?meeting=N` linking one `DownloadAgenda` packet | site: wp paths; BoardBook: 404 | nothing — `links` from the organization page is scoped to `/Public/Organization/` and keeps only the seed |
| Hillsborough County Aviation Authority (TPA) | `tampaairport.com` (Drupal) | its own page; minutes under `/sites/default/files/YYYY-MM/` | Drupal default | nothing. From the box `investigate` reads a sitemap (500+ by convention); from a workstation every page answers 403 |
| St. Pete–Clearwater airport (PIE) | `fly2pie.com` | a county department; its governing body is the BCC | 403, robots.txt included, from both | the BCC by `pinellas-legistar`; the site is blocked |
| HART | `gohart.org` (SharePoint 2013) | `Pages/AboutUS-PublicMeetings.aspx`, packets under `/Board PDFs/` | `_layouts`, `_vti_bin`, `_catalogs` | **blocked (TLS)**: only CBC cipher suites |
| PSTA | `psta.net` (WordPress, sitemap, 410) | Granicus `psta.granicus.com`, view 4: 489 addresses | Granicus: `Disallow: /` | site: nothing yet; meetings need a ruling |
| Tampa Bay Water | `tampabaywater.org` (WordPress) | Granicus `tampabaywater.granicus.com`, view 1: 129 addresses; older years as PDFs on the site | site: none; Granicus: `Disallow: /` | nothing. The site's sitemap does not parse ("unexpected XML declaration at 2:1") |
| THEA (expressway) | `tampa-xway.com` (WordPress, Yoast) | its own page: 32 PDFs under `wp-content/uploads` | empty `Disallow:` | nothing; `sitemap` would, but the page's 32 is past 25 |
| Forward Pinellas (MPO) | `forwardpinellas.org` (WordPress, Yoast) | WordPress Download Manager: `document-portal/<slug>/?wpdmdl=ID&refresh=…&filename=X.pdf` | `/?s=`, calendar variants | nothing; and the `.pdf` is only in the query, so no enclosure scan would take it |
| Plan Hillsborough (MPO/TPO, Planning Commission) | `planhillsborough.org` | SharePoint sharing links, `planhillsborough.sharepoint.com/:b:/…`, 22 on the agendas page | SharePoint: `Disallow: /` | `hillsborough-transportation` (pages); the documents, nothing |
| Tampa Bay Regional Planning Council | `tbrpc.org` | SharePoint folders, `tampabayplanning.sharepoint.com/:f:/s/Public/…`, 151 on the library page | site: empty `Disallow:` and a stray `Crawl-delay: 10`; SharePoint: `Disallow: /` | `tbrpc` (pages); the documents, nothing |

### Fault 1 — the newest Let's Encrypt root, and an error that does not name it

`investigate https://www.southfork3cdd.org/` on the box: `robots.txt unreachable (error
sending request for url …)`, the seed the same, the sitemap the same, and `addresses: 1`. The
server answers a browser. The chain it serves is the leaf and **YR2**, issued by **ISRG Root
YR** — Let's Encrypt's Gen Y root — and nothing that links Root YR to ISRG Root X1:

```text
0 s:CN=www.southfork3cdd.org   i:C=US, O=Let's Encrypt, CN=YR2
1 s:C=US, O=Let's Encrypt, CN=YR2   i:C=US, O=ISRG, CN=Root YR
```

`webpki-roots` 1.0.12, which reqwest's `rustls-tls` compiles in, holds ISRG Root X1 and X2
and not Root YR; neither does the box's OpenSSL store (`curl` there fails the same way). A
browser follows the leaf's AIA address (`http://yr2.i.lencr.org/`) to fetch what is missing;
rustls does not fetch. **10 of the 14 Campus Suite districts serve this chain**; the four
that do not sit behind Cloudflare, which serves its own certificate. None of the authority
sites does — yet. This is the chain Let's Encrypt is moving its subscribers to, so it will
reach sites beyond these.

**HART** fails the same way for another reason: `www.gohart.org` negotiates TLS 1.2 with
`ECDHE-RSA-AES256-SHA384` and refuses every GCM and ChaCha20 suite. rustls implements no CBC
suite. OpenSSL verifies the chain (GoDaddy G2).

Both reach the operator as `error sending request for url`, the outermost layer of the error.
The cause — `UnknownIssuer`, or no shared cipher — is in the error's source chain and is not
printed. *The pipeline measures the fault and does not report it* (Recurring shapes), once
more.

**The lever** is not a strategy. It is a decision about trust roots: add ISRG Root YR to the
client's roots, take roots from the platform verifier, or wait for `webpki-roots` to carry
it. And separately, print the error's source chain. Not built here.

### Fault 2 — a file list is a listing, and the enclosure scan keeps 25 from a page

A Campus Suite district publishes through the product's file manager:
`/files/documents/<folder>/<name>.pdf` on its own host, listed by a `FileList` widget that
renders every folder inline, oldest first, each file an anchor wrapping its title and an
`uploaded October 8, 2026 10:26 AM` line. No Campus Suite site serves a sitemap; robots.txt
is 404, Cloudflare's comment header with no rules, or Cloudflare's managed `Allow: /`. So `sitemap` gives way to `links`, which walks the pages
and names no document, and the enclosure scan takes at most `MAX_PER_PAGE` = 25 from each
page, in page order.

| | South Fork III | K-Bar Ranch II |
|---|---|---|
| pages one click from the front door (calendar left out) | 17 | 17 |
| documents on `/documents` | 102 | 187 |
| documents on `/meetings` | 71 | 157 |
| distinct documents across every page | 109 | 189 |
| what enclosures would keep from `/documents` | the first 25: 2021–2022 agendas | the first 25 |

Across the 14 sites, `/documents` lists **1,893** files (39 to 262 a site), and a walk with
enclosures keeps the oldest 25 to 50 of each — none of 2026. The calendar module shows the
latest five meetings, each with its agenda attached on `campussuite-storage.s3.amazonaws.com`;
at seven districts all 27 of those were copies, by name, of files under `/files/`.

The same cap meets WordPress: Wynnmere East's `/documents` lists 75 PDFs, Newport Tampa
Bay's front door 144, Villages of Westport's front door 102, THEA's meeting page 32. Those are
plain pages, not a product, and want the framework answer, not a strategy.

**Built: `campus-suite`.** Recognised by `Campussuite.Widgets.run(` — on all 14 Campus Suite
front doors and on none of the other ten. It walks the site's pages as `links` does, minus
the calendar (`/calendar` links the next month forever), and keeps every document the pages
link under `/files/` on the site's host as an address, newest upload first; undated inline
links follow in the order found. It keeps none of the storage copies, counts them, and warns
for any whose name matches no kept file. One request per page: 17 at each district measured.
It does not fix Fault 1: ten of the fourteen still cannot be reached.

### Fault 3 — the documents are where robots.txt says no

Three systems publish the pages and forbid the documents:

- **CommunityXS** (GMS's district sites; three sightings): every document is
  `/documents/<id>/download`, served `application/pdf` with a `Content-Disposition` name,
  and robots.txt says `Disallow: /documents/*/download`. Ballentrae's walk on the box
  counted 494 disallowed against 80 pages kept.
- **The `_assets` template** (Water Street Tampa ID, Cory Lakes; two): documents at
  `/_assets/documents/<folder>/<name>.pdf?v=…`, and robots.txt is exactly
  `Disallow: /_assets/documents/`. (Wrathell Hunt's River Hall and WildBlue, outside the
  bay, serve the same template.)
- **SharePoint Online** (Plan Hillsborough, TBRPC; two): `User-Agent: *` / `Disallow: /`,
  and the links are sharing links to folders a script renders.

So Water Street's own district — the reason this entry started — has every agenda, minutes
and budget behind a `Disallow`. `--ignore-robots` is the operator's ruling to make; no
strategy is written for any of the three. A CommunityXS strategy would read the ids off the
page instead of fetching each to learn it is a PDF, which is worth building only once the
ruling is made.

### Smaller, recorded not ranked

- **A sitemap with a blank line before its XML declaration does not parse.** Tampa Bay
  Water: `not valid XML: unexpected XML declaration at 2:1`, so the run falls to `links`.
  One sighting.
- **BoardBook Premier is not collectable by `links`.** The organization page is
  `/Public/Organization/1448` and its meetings are `/Public/Agenda/1448?meeting=N`, outside
  the scope the seed's path draws. One sighting in the bay (BoardBook is a Texas-heavy
  product); a strategy would be the organization page's 22 meetings and one
  `DownloadAgenda` per meeting, with no request between.
- **WordPress Download Manager puts the file name in the query.** Forward Pinellas:
  `…/document-portal/<slug>/?wpdmdl=65348&refresh=<token>&filename=April-2026-CAC-Meeting-Agenda.pdf`.
  The path names no document, and `refresh` changes per view, so `wpdmdl` alone would be
  the address. One sighting.
- **A WAF that answers by network, not by agent.** `tampaairport.com` answers 403 to every
  page from a workstation and serves its sitemap to the box, with the same User-Agent.
  `fly2pie.com` answers 403 to both, robots.txt included.
- **Cloudflare's managed robots.txt** (K-Bar Ranch II) carries `Content-Signal: search=yes, ai-train=no,
  use=reference` for `User-agent: *` and disallows named AI crawlers. Nothing reads content
  signals; for an archive that serves search, `search=yes` is the one that applies.

### Against the three levers

None. Fault 1 is acquisition below the content kind: nothing was fetched to classify.
Fault 2 is enumeration, entry 8's shape again — a document one level down from a listing
— answered by reading the listing. Fault 3 is the publisher's own rule.

### Cost

| Walk | Discover requests | Documents |
|---|---|---|
| `campus-suite`, one district | about 17 | 108 to 262 at the four reachable today; 720 together |
| `campus-suite`, all 14 sampled, once TLS is fixed | about 240 | about 1,900 |
| `granicus`, Tampa Bay Water and PSTA, with a ruling | 2, plus the agenda manager's probe | 129 and 489 addresses |

### Left on the table

- The other ~75 Hillsborough CDDs. The sample says most are Campus Suite; the Commerce list
  would name them, and it is behind a JavaScript challenge.
- Every Campus Suite site Root YR blocks, until the trust-root decision.
- The robots rulings: Water Street Tampa ID, Cory Lakes, the three GMS CommunityXS sites,
  Tampa Bay Water's and PSTA's Granicus, and the two SharePoint libraries.
- St. Petersburg's council and CRA agendas on `cms5.revize.com`. Revize's file hosts appear
  under Largo, Treasure Island and South Pasadena in the crumbs too; a city-council
  question, not this entry's.
- Port Tampa Bay's BoardBook; Forward Pinellas's download manager.
- `PreviousVersions`-like history: a Campus Suite file list says "updated …" beside
  "uploaded …", and a replaced file keeps its address.

### Running log

- 2026-11-22 — box sources and crumbs read: no CDD host among 4,460; PSTA, Forward Pinellas,
  Tampa Bay Water, HART, TPA, PIE and the Clearwater CRA appear only as crumbs.
- 2026-11-22 — Commerce's official list: Azure WAF JS challenge, not read. Auditor General
  index read (1,689 districts, no county). FRS Appendix K: no answer.
- 2026-11-22 — Water Street's district is the WSTID (special act), not a CDD; its robots.txt
  disallows every document.
- 2026-11-22 — 24 Hillsborough and 2 Pinellas districts fingerprinted: 14 Campus Suite, 4
  WordPress, 3 CommunityXS, 2 `_assets`, 2 hand-built, 1 no answer.
- 2026-11-22 — Campus Suite on the box: `error sending request` on every request. Chain ends
  at ISRG Root YR; not in `webpki-roots` 1.0.12 nor the box's store. 10 of 14.
- 2026-11-22 — Campus Suite file lists measured: 39–262 documents on `/documents`, oldest
  first; enclosures keep 25. Storage copies matched by name at seven districts: 27 of 27.
- 2026-11-22 — authorities fingerprinted and investigated on the box: BoardBook (Port),
  Granicus with `Disallow: /` (PSTA, Tampa Bay Water), SharePoint (Plan Hillsborough, TBRPC),
  HART CBC-only TLS, PIE 403, TPA 403 from a workstation only.
- 2026-11-22 — one request broke the measuring rules: a `curl` to
  `ballentraehillsboroughcdd.org/documents/9596/download`, made by hand outside the
  robots-checking script, in the same command that first printed the host's robots.txt —
  which disallows that path. It answered `application/pdf`. No other disallowed path was
  fetched.
- 2026-11-22 — `campus-suite` written; unbuilt and untested here.

---

## 18. Southwest Florida Water Management District — permits and their documents

*Seen 2026-11-22, with plain HTTP requests at one a second or slower against
`www25.swfwmd.state.fl.us` (the District's map services), `www18` and `www38` (WMIS), and
`geoweb.sfwmd.gov` (South Florida WMD), and the box's crumbs. No login; the store was read,
not written.*

The SWFWMD well-permitting agreement on a Manatee County agenda (entry 8) pointed here.
Environmental resource permits (ERP) and water use permits (WUP) are where a development's
wetland and water impacts are decided, and the District files every document of every
application — the application, plans, drainage calculations, staff review, requests for
more information, the permit — in its Water Management Information System (WMIS). No box
source reaches it: the crumbs name `www.swfwmd.state.fl.us` 90 times, all news, water
restrictions and conservation pages, and no permit.

### What is public, and from where

| Host | What | robots.txt |
|---|---|---|
| `www.swfwmd.state.fl.us` | the District's Drupal site: news, the governing board's meeting packets | **unreadable**: every address, `robots.txt` and PDFs included, answers 403 `cf-mitigated: challenge` |
| `www25.swfwmd.state.fl.us` | ArcGIS map services, among them every permit as a feature | 404 |
| `www18`, `www38.swfwmd.state.fl.us` | WMIS: one page per ERP application, one per WUP | 404 at both |
| `swfwmd.granicus.com` | Granicus, presumably the board's video | `User-agent: *` / `Disallow: /`; not fetched |

WMIS's own permit search (`erp/erp/search/ERPSearch.aspx`) is an ASP.NET WebForms
postback. The map service is not. `…/arcgis10/rest/services/Permits/Permits/MapServer`
holds well construction permits (0), ERP (3), WUP applications (4) and WUP issued (5), and
says it is *"only available from 6 A.M. to 10 P.M. daily"* and that public use is allowed.
Each ERP feature carries `ERP_EXT_URL`, the application's WMIS page
(`https://www38.swfwmd.state.fl.us/Erp/ERP/Entry/ERP.aspx?id=1002708`); each WUP feature
`WUP_EXT_URL` (`…/Support/WUP/WupSimple.aspx?st=8833&rev=11`). A query is a GET, pages with
`resultOffset`, answers at most 1,000 features and says `exceededTransferLimit` while more
remain. An unrunnable query answers **200** with `{"error":{"code":400,…}}`, not a status.

The layer: 108,497 ERP features, 106,072 of them project activity areas and 2,425 total land
areas (an application is drawn as one or both). By status: 84,851 approved, 21,175 deleted,
1,457 incomplete, 1,010 denied. There is **no county field**; `PERMIT_DEPARTMENT_NAME` is the
service office (Tampa 36,314, Brooksville 28,752, Sarasota 23,939, Bartow 19,486), which does
not follow county lines. ERP moved to a new system (Appian) for new filings on 2026-07-07,
and the layer still holds them: the newest application was received 2026-11-20, and its
WMIS page (1002708, Manatee) lists its three documents.

### Which keys decide a document

An application's WMIS page is 590 KB to 1 MB, about 440 KB of it `__VIEWSTATE`. Its
Documents tab is pre-rendered in the page, one grid, unpaged — 88 rows on application
732499. Each row's view icon carries an opaque token in its `onclick`:

```text
javascript:ViewDocument([{"Key":"BigSix","Value":"p2avH3Qj9SNwKvPrJjl3UlOnHUFJ…"},{"Key":"MimeType",…}])
```

and the page's script opens `/Erp/Common/Controls/ExportDocument.aspx?OpaqueId=TOKEN`.
Measured on the permit letter of application 921913 (191,777 bytes):

| Request | Answer |
|---|---|
| `ExportDocument.aspx?OpaqueId=TOKEN`, on `www18` or `www38`, http or https | 302 to `/Erp/Export/ViewDoc/<8 random letters>.pdf`, a new name on every request |
| the same, following the redirect, twice | 200 `application/pdf`, the same bytes both times |
| a `ViewDoc/…pdf` a search engine indexed | 404 |
| the parameter as `opaqueid` | the same redirect |
| the token's last character `I`→`J` (padding bits) | the same document |
| one character changed mid-token | 500, "Padding is invalid and cannot be removed." |
| `OpaqueId=` empty | 500 |
| a water use permit's token through `/Erp/…` or `/Support/…` | the same bytes |

The token is an encrypted id, deterministic: application 732499 listed the same 88 tokens
from `www18` at 16:15 and `www38` at 16:32. So the token is the address, and the `ViewDoc`
name is not — collecting through it would store one document under a new address on every
pass. No cookie, no referer. An unknown application id answers 500 ("There is no row at
position 0"); no id at all, 500.

### Water use permits: the documents are behind a postback

`WupSimple.aspx?st=…&rev=…` (121 KB) serves the permit's general tab. Its Documents tab is
`__doPostBack('UltraWebTabWupInfo','4')`: a POST of the page's 30 hidden fields (30 KB of
viewstate) answers 379 KB with the grid — 89 documents on permit 8833, the same token scheme.
No session cookie was sent and none was needed. Collecting them needs a walk that can post a
form; `Walk` is one paced GET.

### What `wmis` does

Seeded with a query on the ERP layer. The query's `where` and `geometry` are the bound; the
walk sets `outFields`, the order (`APPLICATION_RECEIVED_DT DESC, ERP_APPLICATION_ID DESC`),
`returnGeometry=false` and the paging. It reads every page, then opens each application's
WMIS page newest first, each once, and keeps each token as
`https://www38.swfwmd.state.fl.us/Erp/Common/Controls/ExportDocument.aspx?OpaqueId=TOKEN`, on
the host `ERP_EXT_URL` names. It does not keep the WMIS page: two thirds viewstate, and what
it says is in the application and the permit. Water use permits are not collected.

### How a walk is bounded

The layer has no county, so a county is a geometry. One seven-point polygon around
Hillsborough, Pinellas, Pasco and Manatee (west of −82.05 north of 27.646, west of −81.56
south of it, down to 27.21):

```text
geometry={"rings":[[[-82.95,28.48],[-82.05,28.48],[-82.05,27.646],[-81.56,27.646],[-81.56,27.21],[-82.95,27.21],[-82.95,28.48]]]}
&geometryType=esriGeometryPolygon&inSR=4326&spatialRel=esriSpatialRelIntersects
```

Of 28 applications sampled across its last year, 26 named one of the four counties as their
own (9 Hillsborough, 7 Manatee, 5 Pinellas, 5 Pasco); one was Sarasota and one DeSoto, both
on the polygon's south edge. Per-county envelopes overlap each other along Old Tampa Bay and
over-reach more: of 17 sampled from a Hillsborough envelope reaching 27.57, four were Manatee
or Pinellas.

The date is the `where`, and the layer takes a rolling one: `APPLICATION_RECEIVED_DT >=
CURRENT_DATE - 30` answered 122, against 127 for the fixed date thirty days back. So a
scheduled source stays thirty days wide as the weeks pass, and a backfill is a fixed date.

### Cost, at one request a second

Applications in the polygon, deduplicated (1,958 features in the last year were 1,623
applications); documents at the sample's 13.7 an application:

| Walk | Discover requests | Documents to collect |
|---|---|---|
| Last 30 days | 1 query + about 105 pages = **about 106**, 2 minutes | about 1,400, 24 minutes |
| Last year | 2 queries + 1,623 pages = **1,625**, 27 minutes | about 22,000, 6 hours |
| Last five years | 9 + about 7,300 = **about 7,300**, 2 hours | about 100,000, 28 hours (an estimate) |
| Everything (66,145 features) | 67 + about 55,000, 15 hours | not estimated: older applications were not sampled |

A scheduled `centinel run` discovers with the default 200 requests, which reaches the newest
198 applications — about two months here. The thirty-day source fits inside it.

### The governing board

Its meeting packets are PDFs on the Drupal site
(`/sites/default/files/calendar/March%20Web%20Notebook_2.pdf`), and the notices say agendas
go up a week ahead under "Meetings & Events Calendar". The whole site answers a Cloudflare
managed challenge to a plain client, so every address there collects as a 403 — Blocked, not
Gone. `swfwmd.granicus.com` exists and refuses every crawler; the `granicus` strategy would
read it only under an `ignore_robots` ruling, and without one its view ids were not looked
up. So nothing for the board is collectable today, and nothing was built.

### A second sighting: South Florida WMD

`geoweb.sfwmd.gov/agsext1/rest/services/Regulation_ApplicationPermits/Environmental_Resource_Permit/FeatureServer`
(no robots.txt): twelve application layers by type. A feature carries `COUNTY`, and `LINK`
to the application in SFWMD's own ePermitting (`my.sfwmd.gov/ePermitting/SearchPermit.do?…`),
a different product from WMIS. So the shape that recurs is **a district's GIS layer that
names each permit's record page** — two sightings, and the cheap way into both permit
systems. What sits behind the link is per product: WMIS's grid here, ePermitting there, not
measured. St. Johns River WMD was not checked.

### Against the three levers

None. Enumeration: the documents are PDFs that read, and the only obstacles are a
postback search (bypassed through the map service) and a document address that changes on
every request (bypassed through the token that produces it).

### Left on the table

- **Water use permit documents**, one POST per permit. The smallest design is a second method
  on `Walk` (a paced, budgeted POST of a form), written out in the pull request.
- The WMIS page's own record (status, county, conditions tab, dates): not kept, and the
  layer's attributes say most of it; worth a look at what extract makes of the page.
- WUP record pages (`WupSimple.aspx`), GET and 121 KB: permitted quantities and withdrawal
  points.
- Well construction permits (layer 0), self certifications (1), exemptions (2).
- The governing board, if the Cloudflare challenge ever lets a plain client through, or under
  a ruling on Granicus.
- SFWMD's ePermitting behind `LINK`.

### Running log

- 2026-11-22 16:13 — robots.txt: main site challenged (403), `www18`/`www25`/`www38` 404,
  `swfwmd.granicus.com` disallows all. Box crumbs: 90 links to the main site, none to WMIS.
- 2026-11-22 16:15 — ERP page 732499 (995 KB, 88 documents); the grid carries `BigSix`
  tokens; `ExportDocument.aspx?OpaqueId=` redirects to a fresh `ViewDoc/*.pdf` each time,
  same bytes; tampered token 500.
- 2026-11-22 16:20 — map service `Permits/MapServer` layers 3 and 5 carry `ERP_EXT_URL` /
  `WUP_EXT_URL`; no county field; 108,497 ERP features; Appian-era filings still listed.
- 2026-11-22 16:22 — WUP Documents tab is a postback; one POST, no cookie, 89 documents,
  same tokens, served through `/Erp/` too.
- 2026-11-22 16:25 — Hillsborough envelope: 644 applications in a year, 17.3 documents a
  page over 17, 4 of 17 outside the county.
- 2026-11-22 16:30 — SFWMD's ERP feature service: `COUNTY` and `LINK` per feature, a
  second sighting of the GIS-to-record shape.
- 2026-11-22 16:35 — Tampa Bay polygon: 1,623 applications in a year, 26 of 28 inside the
  four counties, 13.7 documents each; rolling `CURRENT_DATE - 30` accepted. `wmis` written;
  unbuilt and untested here.

## 20. Money and influence — lobbyists, campaign finance, financial disclosures

*Seen 2026-11-22, with plain HTTP requests at one a second or slower under Centinel's user
agent, robots.txt read first, no login and no registration; and with the box's
`log/*/source.toml`, logs and crumbs, read and not written.*

Who is paid to talk to a government, who pays for its campaigns, and what its officials
own. Ben ruled the personal data in scope: collected as published, never enriched,
cross-indexed or assembled into a profile of anyone.

### Where each record set lives

| Record set | Publisher | Where | Without a login | On the box |
|---|---|---|---|---|
| Lobbyist activity reports, quarterly | Hillsborough County Attorney | PDFs on `assets.contentstack.io`, listed by `hcfl.gov`'s sitemap | yes | **yes**: 39 reports, 2016 Q3 to 2026 Q3, in `hillsborough` |
| Lobbyist registrations | Hillsborough County | LIRA, Oracle APEX at `public-solutions.hillsboroughcounty.org/enterprise/f?p=370` | no: a login form | no |
| Lobbyist meeting sign-in | Hillsborough County | APEX `f?p=321`, a kiosk page | a sign-in form, no records | no |
| Registered lobbyists, by year | City of Tampa | `apps.tampagov.net/Lobbyist_log_webapp/default.aspx?Page=RegisteredLobbyists` | yes, page 1 by GET | no |
| Lobbyist and visitor meeting log | City of Tampa | `…/default.aspx?Page=RequestedMeetings` | yes, page 1 by GET; all of it by POST | no |
| Lobbyist expenditure report form | City of Tampa | `tampa.gov` | yes | yes, in `tampa` |
| Registered lobbyists, 2024–2026 | Pinellas County | `pinellas.gov/20NN-registered-lobbyists-for-pinellas-county-government/` | yes | no: `pinellas.gov` is not a source |
| Registered lobbyists, older years | Pinellas Clerk | PDFs on `mypinellasclerk.gov` | Cloudflare challenge | no |
| Lobbyist registrations | St. Pete Beach | CivicPlus Document Center, folder 313 | yes | the page about them; **no file** |
| Lobbyist registration | Clearwater | redirects to a Smartsheet form | no list found | — |
| Campaign treasurer's reports | Hillsborough, Pinellas, Pasco, Manatee SOEs | VoterFocus, `www.voterfocus.com/CampaignFinance/` | yes | no |
| Form 1 and Form 6 | Florida Commission on Ethics | EFDMS, `disclosure.floridaethics.gov` | yes | no |
| Candidates' Form 1 and Form 6 at qualifying | each SOE | VoterFocus, as scanned PDFs | yes | no |

Not found: a lobbyist registry for St. Petersburg. Not measured: Tampa International
Airport's (HCAA Policy P156) list of registrations.

### Lobbyists

**Hillsborough County is collected already.** The County Attorney publishes a quarterly
*Lobbyist Activity Report* PDF; `hcfl.gov`'s sitemap lists each one and the box's
`hillsborough` source holds 39, from 2016 Q3 to 2026 Q3, two quarters absent (2024 Q3 and
2026 Q1). The page that presents them (`/government/codes-and-ordinances/lobbying-information`)
renders its table in the browser and serves "No data available" to a fetch; the sitemap is
what reaches them. Registrations themselves are in LIRA, behind a login, and the kiosk
sign-in page records nothing a visitor can read.

**Tampa's log is a Telerik RadGrid, and the whole of it is one POST away.** The app's two
reports answer a GET with their first page and a footer: *221 items in 23
pages* for 2026's registered lobbyists (a year menu reaches back to 2017), and *6074 items
in 304 pages* for requested meetings. The pager, the year menu and the Excel and PDF
exports are `__doPostBack` calls, the shape entry 3 met at CTTV on the same host. One POST
of the meeting log's Export to Excel, carrying only the hidden fields of the GET it
followed and no cookie, answered `CityOfTampaLobbyistMeetingLog.xls`: 2,029,077 bytes, an
OLE workbook of 6,074 rows, eight columns (date, employee, lobbyist or visitor, principal,
principal's business, subject, cancelled), from 2014-05-18 to 2026-11-23. 2,675 of those
rows are *visitors*, not lobbyists: residents meeting staff about code enforcement or a
garnishment, by name. robots.txt disallows nothing under `/Lobbyist_log_webapp`.

**Pinellas County publishes a page a year** on `pinellas.gov`, 2024, 2025 and 2026, each a
table of every registered lobbyist with an address and the principals they represent (32
rows in 2026). The Yoast sitemap lists all three among 3,008 pages, and robots.txt allows
them. Lobbyists file meeting logs by a form; no published log was found. Older years are
PDFs on the Clerk's site (2020, 2021 and 2023 named by a search), which answers our client
with Cloudflare's "Just a moment" challenge, 403, while its robots.txt allows `*`.

**St. Pete Beach files each registration as a PDF in its Document Center**
(`/DocumentCenter/Index/313`; the 2026 list at `/DocumentCenter/View/7167/Lobbyists-Annual-Registration-2026`).
The box's `st-pete-beach` source holds the page that links them and none of them, and that
is the fault below, not a choice.

### Fault — no CivicPlus box source holds a single Document Center file

The eight CivicEngage sources of entry 9 hold, between them, **zero** observations under
`/DocumentCenter/View/`: St. Pete Beach 0 of 339 observations, and Indian Shores, Oldsmar,
Pinellas Park, Safety Harbor, Seminole, Tarpon Springs and Temple Terrace the same. A
Document Center link names no extension — `/DocumentCenter/View/7167/Lobbyists-Annual-Registration-2026`
— and the enclosure scan keeps only a path ending in an extension a reader claims
(`enclosure::is_document`), so the file one level away is never fetched. The CivicPlus
sitemap does not list them either. It is the **enclosure** lever, and it reaches every
CivicEngage site. Not built here: either the enclosure test learns that an address can be
a document without saying so (it cannot know until it fetches), or a `document-center`
strategy walks `/DocumentCenter/Index/<folder>`, which entry 9 already left on the table for
Oldsmar and Pinellas Park.

### Campaign finance: four counties, one vendor

`votehillsborough.gov` and `votepinellas.gov` answer our client with Cloudflare's challenge,
403, robots.txt included. It does not matter: both send a reader to VoterFocus, and so do
Pasco and Manatee, all on one host with the county as a parameter. Hernando, Polk and
Sarasota answered the same page.

| County | Reporting groups | Filers | Oldest group |
|---|---|---|---|
| Hillsborough | 27 | 2,264 | General Election of 11/7/2000 |
| Pinellas | 15 | 382 | 2016 candidates; committees from 2007 |
| Pasco | 24 | 1,655 | General Election (11/7/2006) |
| Manatee | 169 (8 empty) | 1,781 | 2002 |

A **reporting group** is an election cycle, a municipal election, or the committees or
parties reporting through a date. Every group is an `<option>` of a GET form, and each
answers all its filers on one page, unpaged: Hillsborough's 2026 cycle links 224 filers and
its candidate CSV has 224 rows. The largest page was 454 KB. No filer is listed in two
groups. A filer is a candidate, a political committee or a party.

**What decides each document**, measured on Hillsborough filer 2621:

| Address | Keys that decide it | What the others do |
|---|---|---|
| `candidate_pr.php?c=C&el=G` — a group | `c`, `el` | |
| `candidate_pr.php?op=cv&e=G&c=C&ca=N` — a filer's page | `c`, `ca` | `e` and `committee` are copied into the page's links; without `c`, 14 bytes; another county's `c`, 200 with an empty body |
| `candidate_pr.php?op=rp&e=G&c=C&ca=N&sdc=S&dhc=D` — one report | `c`, `ca`, `dhc` for the rows; `e` and `sdc` for the PDF link | another filer's `ca`: 200, an empty report dated 12/31/1969; without `e` or `sdc` the page links no PDF; `rellevel` changes nothing |
| `pdf_C/e27c2621_Q2_dhc13282sdc1039.pdf` — the report as filed | the file name | another `e` in the name: a real 404 |
| `pdf_C/doc_2621_20260611111456_Financial_Disclosure.pdf` — a qualifying document | the file name | |
| `export.php?op=CFINANCE&cand_id=N&dhc=0&county=C` — every transaction, CSV | `cand_id`, `county`; `dhc` picks one report, `0` all | `rptStatus` changes nothing |
| `export.php?op=candidates&county=C&election=G` — the group's filers, CSV | | |

So the walk asks each filer's page with its group, keeps every report link with `op`, `e`,
`c`, `ca`, `sdc` and `dhc` as read, and drops `rellevel` and `committee`. The filed PDF and
the qualifying documents are taken off those pages by acquisition, as enclosures. The CSV
for filer 2621 is 363 rows over 19 reports: every report on the page but a $0 amendment.

**A report as filed** (998,421 bytes, three pages, Helvetica and Courier) has a text layer.
**A qualifying Form 1 or Form 6** (54,426 bytes) is a scan with no font, so it reads as
nothing until an OCR reader exists. Forty filers sampled across the four counties held 6.05
reports and 4.5 qualifying documents each; 18 of the 40 had filed no report.

### Fault — every VoterFocus CSV would have been Underivable

Both exports answer `application/vnd.ms-excel` over plain CSV, the usual way to get Excel to
open one. `ContentKind::classify` believes a declared type it knows, so these were
`spreadsheet`; the one spreadsheet reader, calamine, opens OLE and ZIP workbooks and
answers anything else with "spreadsheet parse failed", which records an Underivable. That is
the **content kind** lever. Fixed in this change: a declared spreadsheet over bytes that
prove no format (`from_magic` is `None`) and hold no NUL is `csv`, and reaches the
passthrough reader. A real workbook under the same header, such as Tampa's OLE `.xls`, stays
a spreadsheet; so do empty bytes.

### robots.txt and pace

`www.voterfocus.com/robots.txt` answered **503**, "This service is temporarily unavailable
for maintenance", on three tries over about twenty-five minutes, while every page
answered 200. RFC 9309 lets a crawler treat a 5xx as *disallow everything*; Centinel's host policy
(`unreachable_robots`) defaults to allow, and the strategy leaves that call to it. No
`Crawl-delay` was ever readable. `disclosure.floridaethics.gov/robots.txt` redirects to the
site's own "404 Not Found" page, answered **200** `text/html`, which parses as no rules.

### Cost, and how a walk is bounded

Discovery is one request per group and one per filer. Collection is the filer's page, its
CSV if it reported, each report page with its PDF, and each qualifying document: about 18
requests a filer at the sample's means, an estimate. At one request a second:

| Walk | Discover | Collect, estimated |
|---|---|---|
| Hillsborough, every group | 27 + 2,264 = **2,291**, 38 min | about 41,000, 11.4 h |
| Pinellas, every group | 15 + 382 = **397**, 7 min | about 7,000, 1.9 h |
| Pasco, every group | 24 + 1,655 = **1,679**, 28 min | about 30,000, 8.4 h |
| Manatee, every group | 169 + 1,781 = **1,950**, 33 min | about 32,000, 9 h |
| Hillsborough, 2026 cycle (`&el=27`) | **224**, 4 min | about 4,100, 1.1 h |

All four counties are about 6,300 discovery requests and 110,000 collection requests, some
31 hours of one host. A scheduled run's default 200-request budget reads every listing of a
county and then the filers' pages in the order the groups are listed, the current group
first, so it keeps every filer's page and the newest reports. A backfill is a `discover
--max-sitemaps N` with N above the discovery figure.

### Financial disclosures: one publisher, an open JSON API

EFDMS's public search is a JavaScript page over GET endpoints that need no token:

1. `api/PublicFiling/SearchPublicFilers?filterByFormYear=2025&pageSize=50&pageNumber=1` —
   JSON, 40,065 filers for form year 2025 and 117,986 with no filter, paged.
   `filterByFormTypeCode=6` narrows 2025 to 2,904 Form 6 filers. `filterByCountyId=39`
   (Hillsborough; Pinellas 62, Pasco 61, Manatee 51) is echoed back and narrows nothing; the
   county search's real parameters were not found.
2. Each filer carries `filingHistoryUrl`, `PublicSearch/FilingHistory/<userId>`, another
   shell; its data is `api/PublicFiling/SearchFilingHistory?userId=<userId>`, one row a form
   year.
3. A row filed electronically carries `printUrl`, `Report/PrintForm/?filingId=<id>`, a viewer
   whose PDF is `Report/GetFormContent?filingId=<id>`, a redirect to
   `api/Report/RenderPdf/<id>/False`: 544,039 bytes, four pages, with a text layer. An
   unknown `filingId` answers 200 with an empty body.
4. A Form 1 filed before form year 2023 went to the county SOE: its row says
   `isScannedForm`, carries no URL, and says to contact the Commission. Those are not
   online here. The old search, `public.ethics.state.fl.us`, timed out.

The rows also name each organisation's coordinator with an email and a telephone. One
publisher, so no strategy. If a second state's system turns out to be this product, the
walk is plain: filers by form type and year, each filer's history, each `GetFormContent`.

### Personal data, by source

For a later rule that keeps some of these out of search:

| Source | Private individuals named | With addresses |
|---|---|---|
| VoterFocus report pages, report PDFs, transaction CSVs | every contributor, with occupation | **street address** |
| VoterFocus candidate CSV (`op=candidates`) and filer pages | candidates | **home address**, email, telephone |
| VoterFocus qualifying documents | candidates; Form 1 and Form 6 list their finances | home address on the forms |
| Tampa meeting log | 2,675 visitor rows: residents meeting staff, by name, with the subject | no |
| Pinellas lobbyist pages | lobbyists, as registrants | business address |
| EFDMS forms and API | officials and appointees: income sources, real property, liabilities | home address, except filers exempt under s. 119.071 |
| EFDMS API | organisation coordinators | work email, telephone |
| Hillsborough activity reports | lobbyists and principals | — |

### Against the three levers

- **Enumerate**: VoterFocus. Built, `voterfocus`.
- **Content kind**: CSV declared as Excel. Fixed.
- **Enclosure**: Document Center links name no extension. Recorded, not fixed.

### What a POST walk would need (Tampa), recorded not built

`Walk` is one paced GET. The meeting log, the lobbyist list per year and CTTV's transcripts
(entry 3) are all one ASP.NET postback away, and two sightings on one publisher is one
sighting by this file's rule, so nothing is built. What was measured narrows the design:

- The POST needs **no cookie and no session**: only the hidden inputs of the GET it follows,
  plus `__EVENTTARGET`. So the state is in the page, not in a jar.
- Its answer has **no address of its own**: the `.xls` is what a URL plus a body produced.
  A Resource is an address, so it needs one, the way a feed entry is `<feed>#entry=<id>`:
  `…/default.aspx?Page=RequestedMeetings#postback=RadGridLogReport$ctl00$ctl02$ctl00$RadGridLogExportToExcel`.
- The view state is per response, so the POST cannot be done at discovery and replayed at
  collection. Acquisition owns it: an address with `#postback=` is a GET of the page without
  the fragment, then one POST of its hidden fields with that target, paced and counted as two
  requests. One capability in `fetch`, not in any strategy, and no strategy has to hold state.

A third sighting on another publisher should be checked against that before it is built.

### Left on the table

- Tampa's two RadGrid reports, behind the design above.
- Older Pinellas registrations and Clerk records behind Cloudflare's challenge.
- An OCR reader for the scanned qualifying Form 1s and Form 6s.
- EFDMS, one publisher with an open API.
- The filer's name as the title of its report and CSV: the listing names both.
- The report pages are kept to reach their PDFs, so each report is in the corpus as HTML
  and as the filed PDF, and its rows again in the filer's CSV.

### Running log

- 2026-11-22 — robots.txt read at nine hosts; VoterFocus 503, two SOE sites behind
  Cloudflare. Tampa's RadGrid measured; one POST of the export returned all 6,074 meetings.
- 2026-11-22 — Hillsborough LIRA is a login; its 39 quarterly reports are on the box.
  Pinellas' three yearly pages found in its sitemap; the Clerk's PDFs behind Cloudflare.
- 2026-11-22 — VoterFocus keys measured at Hillsborough; every group of four counties counted
  (235 requests, 6,082 filers); 40 filers sampled. Found the CSV classification fault.
- 2026-11-22 — EFDMS's GET API traced from search to PDF.
- 2026-11-22 — Document Center gap found at St. Pete Beach, confirmed at all eight
  CivicEngage sources on the box.
- 2026-11-22 — `voterfocus` strategy and the CSV classification fix written; unbuilt here.

---

## Recurring shapes

A shape is promoted here when **two** sites show it.

### The one every entry shows

**A source is a tree of systems, and the tree crosses hosts.** — *4 sightings of 4*

*Entry 5 does not test this and does not contradict it. Those six were run as whole sites,
not investigated for handoffs; the one that was checked, `buffalony.gov`, dropped five
plausible crumbs — a meeting vendor, a records vendor, an open-data portal, an assessment
vendor, and CivicPlus's own portal. The claim below still rests on entries 1–4.*

Not one entry in this file lives where its `.gov` domain does. Every single one is a
different host, reached by a link, invisible to any sitemap:

| Entry | The content | Its host | Reached from |
|---|---|---|---|
| 1 | FY2027 budget book | `stories.opengov.com` | `tampa.gov` |
| 2 | agendas and minutes | `tampagov.hylandcloud.com` | `tampa.gov/city-clerk` |
| 3 | 2,606 council transcripts | `apps.tampagov.net` | `tampa.gov` |
| 4 | ~6 GB of bulk court data | `publicrec.hillsclerk.com` | `hillsclerk.com` → HOVER → there |

And the sitemaps confirm the systems cannot see each other:

```
tampa.gov/sitemap.xml    → 6 entries, all www.tampa.gov
hillsclerk.com/sitemap.xml → 182 child sitemaps, all www.hillsclerk.com
```

**The `tampa.gov` source in the store holds 19,134 observations and cannot reach any of
entries 1, 2 or 3.** Not because discovery is broken — because those addresses are not in
the sitemap it enumerates, and never will be. `SiteSource` already knows a *discovery run*
can span hosts; its own doc comment cites `hcfl.gov`'s sitemap being advertised by
`hillsboroughcounty.org`. But that is one sitemap naming another host. Here the other host
is named in a **page link**, and nothing follows those.

That is why entry 4 took three hops by hand — `hillsclerk.com`, then HOVER, then
`publicrec` — and why the best source in this file was two clicks from a page a crawler
would have read and discarded.

#### The direction this points (Ben's, 2026-08-07, recorded not decided)

Add a step that maps *sources*, not pages. Something like `centinel investigate <domain>`:
walk a domain looking for the **other systems it hands off to**, and report a tree of
candidate sources for a person to accept or reject. Deliberately not a crawler — it does not
follow links to find content, and it does not recurse into a link graph with no bound. It
looks for handoffs, then stops and asks.

Why this shape rather than "follow more links":

- It keeps `enumerate`'s promise intact. `CONTEXT.md` rejects a second level of enclosure
  because *a second level makes acquisition a recursive crawler with no snapshot to bound
  it*. The same objection kills link-following at discovery. A tree of **sources** has no
  such problem: each node is a Source, and each Source still enumerates its own complete
  snapshot.
- It puts the human where the judgment is. Every entry in this file needed a decision a
  crawler cannot make: *collect the PDF not the HTML*; *do not crawl HOVER at all*; *this
  6 GB is a person-level index and wants a deliberate yes*.
- It matches how the work actually went. Four sites, four times the shape was: find the
  handoff, judge it, then enumerate. The judging step is the one with no code behind it.

#### Crumb — the word for it (Ben, 2026-08-08)

**Crumb** — an off-host link seen during a DiscoveryRun, recorded and not followed.

A crumb holds four facts: the host it points to, the address that carried it, the time, and
how many links pointed the same way. The operator lists crumbs from the CLI, then promotes a
host to a Source or ignores it.

**It is fractal.** A new Source walks its own host and drops its own crumbs. The operator
promotes again. This repeats until the crumbs are only footer links to Facebook.

**The recursion stops because a person cuts it, one time per host.** This is the whole
answer to "do not loop". Rule 2 stops the walk at the host edge. A crumb records the edge
instead of crossing it. A person crosses it, deliberately, and gets a Source with its own
cadence and its own DiscoveryRun.

**A crumb is derived, not truth.** Correcting what this file said earlier. A crumb is a link
read out of a page, and that page is a blob. So a crumb rebuilds from `blobs/` and belongs
in `centinel.db` with the other derived views.

The *verdict* on a crumb is different. "The operator ignored `facebook.com`" cannot be
rebuilt from any blob. It is new information, so it is truth, and it belongs in the log.
Without it, every run re-offers every host the operator already rejected.

##### What the operator types

The commands sit beside `source` and `schedules`, and use the same shapes:

```
centinel crumbs                        # every host found, newest first
centinel crumbs --source tampa         # only what one Source dropped
centinel crumbs show apps.tampagov.net # the addresses that linked there
centinel crumbs ignore facebook.com    # write the verdict; stop offering it
```

```
HOST                          LINKS  FOUND BY  STATUS
apps.tampagov.net               412  tampa     new
tampagov.hylandcloud.com         88  tampa     new
stories.opengov.com              31  tampa     new
hillsclerk.com                    4  tampa     promoted
facebook.com                      2  tampa     ignored
```

Promotion needs no new command. `source add` already does it:

```
centinel source add cttv --site https://apps.tampagov.net/cttv_cc_webapp/
```

Three states cover the whole cycle: `new`, `promoted`, `ignored`. Only `ignored` and
`promoted` are truth — they are the operator's decisions. `new` is the absence of a
decision, so it is derived along with the crumb.

**The link count is the judgment aid.** 412 links to one host is a system. Two links is a
footer. That number is what lets an operator scan the list instead of study it.

##### Where this sits against the stages

`crumbs` is a **product of discover**, not a stage of its own. The fan-out already reads
every link on every page it walks. A crumb is the link it decided not to follow. Recording
it costs one row and no extra fetch.

This keeps `CONTEXT.md`'s rule that stages stay separate. Nothing new runs. `discover`
learns to write down what it refused, and a new command reads that back.

##### What was built, and the four places it diverged (2026-08-10)

`centinel crumbs`, with `show`, `ignore` and `allow`. The design above survived; four of its
mechanisms did not, and each was decided by what the code turned out to hold.

**1. A product of `collect`, not of `discover`.** The sentence *"the fan-out already reads
every link on every page it walks"* describes a strategy that does not exist: `sitemap`
enumerates from sitemap XML and `listing` from directory indexes, and neither reads a content
page's links. The stage that *does* is `collect` — `enclosure` scans every stored page's `<a>`
tags for the documents it carries, and drops the off-host ones on the floor at
`enclosure.rs:99` with no count. So the row is written there, from markup already in memory
whose kind is already classified. The instinct in this entry was right about *when* (the moment
the page is in hand, no extra fetch) and wrong only about *which stage* is holding it.

**2. A file, and it is worth what it costs.** `crumbs/<source>.jsonl`, one row per stored
artifact. Measured on 5,000 pages of 91 KB — 449 MB of blobs:

| | |
|---|---|
| read every blob | **8.8s** |
| read the ledger | **0.05s** |
| the ledger | 2.3 MB |

A first draft of this feature had no file at all and re-read every blob on every pass, on the
argument that a row would be a second copy of a fact already on disk. The argument is sound and
the conclusion was wrong: the blob is the *authority*, and the row is a cache of a parse of it —
so the blobs stay the floor. A missing row costs one blob read, a damaged file is repaired by
`--rescan`, and the pass reports how many pages it had to read that way. Slow is a fine failure
mode. Quietly incomplete is not, and that is the only reason the count is on the report.

**2b. `centinel.db` was the wrong home for it.** A row would have coupled `collect` — a paced
network stage — to the search index, which `index` owns and which is rebuilt wholesale. A file
per Source keeps the stages separate and keeps one city's crumbs separable, exactly as `log/`
does.

**3. The ruling is in `decisions.jsonl`, not in `log/`.** "It belongs in the log" was right
that it is truth and wrong about which file. `log/` is per Source, and *"this host is not a
Source"* is not a fact about Tampa: filed per Source, `facebook.com` would need refusing once
per city, and a Source added next year would re-offer every host already rejected. So the
rulings are corpus-wide, append-only, latest-wins — which is what lets `allow` reverse an
`ignore` without erasing the record that it was made.

**4. `promoted` needs no record at all.** The three states are `open`, `ignored` and
`collected`, and only the second is stored. The third is read off the log: an Observation on
that host **is** the promotion, so nothing has to be written and nothing can disagree with
the corpus. Collected also beats ignored, because a host that became a Source is answered and
an old refusal on it is stale rather than binding.

The link count did the job it was given. Three pages of one city's site produced five hosts
sorted by link count, and the two at the bottom were a font CDN and a social network — the
list is scanned, not studied. A **page** count sits beside it: one page linking a host twenty
times is a widget in a template, and twenty pages linking it once each is a system.

#### The shape, sharpened (Ben, 2026-08-07)

Within a source, keep the fan-out — follow same-host links. Links that leave the host are
**not** followed; they are written to an *investigate table* as candidates, and a person
decides which become sources. One system, one source: `tampa` is `tampa.gov`; the city clerk's
records are their own source with their own cadence.

Checked against all four entries, three things fall out.

**1. Exact hostname, not registrable domain.** Entry 4 is the evidence and it is decisive:

```
www.hillsclerk.com   → CMS, sitemap, ordinary pages
hover.hillsclerk.com → search-only; must not be crawled at all
publicrec.hillsclerk.com → ~6 GB of bulk files, a directory walk
```

One registrable domain, three systems, and entry 4's whole conclusion is that they need
three *different* answers. Match on the registrable domain and fan-out silently swallows all
three into one source, including the one that should never be crawled. Match on the exact
hostname and they naturally arrive as three candidates for a person to judge separately —
which is the right outcome, reached by the simpler rule.

It also agrees with what the codebase already says. The enclosure rule is *one level, **same
host***. Using the same test at discovery keeps one definition of "elsewhere" instead of two.

**2. Fan-out finds nothing on half the entries, and that is worth knowing rather than
fixing.**

| Entry | What same-host `href` fan-out would find |
|---|---|
| 1 — OpenGov | **0.** Addresses are in `data-public-url`, not `href`. |
| 2 — OnBase | **0.** Addresses are a JSON literal inside a `<script>`. |
| 3 — CTTV | **50 of 2,606.** Real `<a href>` rows, but the pager is `__doPostBack`. |
| 4 — publicrec | **everything.** A directory listing is nothing but links. |

So fan-out is cheap, correct, and the right default — and it is not a substitute for a
source's own `enumerate`. The useful consequence: **a fan-out that returns very little is a
signal, not a finished job.** Entries 1 and 2 would report zero same-host addresses from a
page that plainly lists dozens; that is exactly the moment to tell a person the source needs
a real enumerate, rather than recording a healthy run over one wrapper page.

Note the tension fan-out inherits: entry 4's `publicrec` walk is unbounded from the outside,
and `CONTEXT.md` forbids a silently capped snapshot. Whatever bounds the fan-out has to say
so, the way entry 3's grid says *"2606 items in 53 pages"*.

**3. The investigate table is an observation, not a plan.** A row says: *this address, on
this page, at this time, pointed off-host to that host.* That is evidence, append-only, and
it sits on the truth side of the store. Promoting a row to a source is a separate, human
act — which keeps the judgment where every entry in this file showed it was needed: collect
the PDF not the HTML, never crawl HOVER, decide about the 6 GB deliberately.

#### The staggering already works — splitting sources is what unlocks it

Worth stating plainly, because it changes what has to be built. `schedule set` already takes
a repeatable `--source` and its own cron, and `run` already takes `--source`:

```
centinel schedule set tampa-daily  --cron "0 2 * * *" --source tampa
centinel schedule set clerk-hourly --cron "0 * * * *" --source hillsclerk-publicrec
```

Per-source cadence, staggered, independent. None of that needs writing. Today there is
simply nothing to stagger, because one site is one source. **The cheaper-runs benefit is a
consequence of splitting the tree, not a thing to build alongside it** — and `publicrec`,
which changes daily and rolls files off after two to four weeks, wants a different cadence
from a CMS that changes monthly, right now.

#### Still open

- **What signals a handoff?** Off-host is the mechanical test. Entries 1–3 fit it directly;
  entry 4 fits once the test is exact-host. A directory listing, a search form and a viewer
  shell are all *evidence about what kind of system was found*, and a person judging a
  candidate would want that on the row.
- **What does a candidate row record?** Enough to judge without fetching: the host, where it
  was linked from, how often it was linked, and whether an existing source already covers it.
- **Does a source ever span hosts?** Entry 4 says a domain does not equal a source. Whether
  the reverse happens — one system across two hostnames — has not been seen yet.

The catalogue's rule applies here too. Four sightings is well past the promotion bar, but
this is a **direction**, not a design. It should meet a county that is neither Tampa nor
Hillsborough before anything is built.

### Promoted

**The pipeline measures the fault and does not report it.** — *4 sightings: clevelandohio,
medina, dunedin, buffalony — and it is the shape entry 5 is really about*

Every large fault in entry 5 is a number that already exists somewhere in the process and
never reaches the person running it. `check` computes a link-share verdict and `run` drops it.
The fallback measures readability's yield against the whole page's, uses it once for a
threshold, and discards it. `build_index` knows a document produced no chunks and returns
`continue`. `investigate` knows its probe hit a cap and prints a checkmark anyway.

This is a sharper version of what `CONTEXT.md` already forbids for a truncated DiscoveryRun —
*a truncated snapshot looks exactly like a source that shrank* — and the four sightings say
the rule was written too narrowly. It is not only snapshots. **Any stage that stops early,
strips something, or scores a read badly owes that fact to its report.** A silent success is
worse than a loud failure, because only one of the two gets investigated.

**The extractor chooses a region, and on a sparse page it chooses wrong in both
directions.** — *4 sightings: clevelandohio (385 pages), medina, dunedin, boston*

Readability picks a content region by scoring. On a page with a real article it is right and
the output is clean. On a page whose content is a short fact block — a landmark's year and
architect, a meeting's date and venue — it either picks a *different* dense block (a contact
panel, a site-wide banner) or finds too little and hands the whole page to the fallback,
navigation included. Both outcomes score as success.

The two failures are the same decision seen from either side, and the deciding number is a
fixed character floor. What the sightings add over one is that **the floor is the wrong kind
of test**: 378 chars is plausible for a short page and catastrophic for a 132 KiB one.

The direction they do *not* support is a ratio in its place — measured across 300 documents,
readability's share of the whole page separates nothing, because good reads run 0.101–0.944
and bad ones 0.003–0.583. What holds is weaker and true: **a fallback must be an
improvement**, tested with the link share that already earned its place. Only the first of
the two failures is fixed by it; picking the wrong dense region is untouched, and still has
no test but a person reading the page.

**A limit meant for one stage silently binds the others.** — *4 sightings:
clevelandcitycouncil, clevelandohio, dunedin, boston*

`--limit` is documented as a collection cap and is threaded into extract, transcribe and
embed. On `boston.gov` this made every one of 161 PDFs invisible while the report read clean,
because address ordering put HTML first. The general shape: **a cap applied to a stage that
does no fetching is not a politeness bound, it is data loss**, and the stage that inherited
it had no reason to.

**The address set lives on the page, and not in a link.** — *2 sightings: OpenGov Stories,
OnBase Agenda Online*

Both sites hold their complete address set in the shell HTML, and in both cases an `href`
scan finds none of it. OpenGov puts it in a `data-public-url` attribute; OnBase puts it in a
JSON literal inside a `<script>`. Neither site has a sitemap that answers.

This is now the strongest candidate for a real build. `SiteSource` can only enumerate by
walking a sitemap, so both sites are uncollectable for a reason that has nothing to do with
their content. What the two sightings add over one is the shape of the seam: **where on the
page the addresses live is the thing that varies**, and it varies a lot — an attribute on an
anchor, a field in embedded JSON. So the lever is not "read the links"; it is "a source
declares where its addresses are". A third sighting should be checked against that
prediction before anything is built.

**The title and the body are at different addresses.** — *3 sightings: OnBase Agenda Online,
CTTV captions, and `tampa.gov` in the record already*

The document's name is at the address that referred to it; the document is somewhere else,
and carries no name of its own. OnBase's `ViewAgenda` returns a Word fragment with an empty
`<title>` and letterhead `<h1>`s identical across every meeting ever held; its PDF has no
`Title` metadata either. CTTV's older transcripts have speech but no parseable date header —
the date is in the index row, behind the pager.

`CONTEXT.md`'s **Title** rule says the name is in `<title>`, `og:title` and `<h1>` and
nowhere in the body, so it is written in as an `# H1` to reach every chunk's heading path.
Three sightings say the rule holds for a CMS page and fails for a content endpoint. Both
failures are silent: no title is simply no `# H1`, and the chunks sit under a heading path
that cannot tell one meeting from another.

The fix that covers all three: **when a source enumerates from structured data, the title
comes from that data.** OnBase's search JSON holds `Name` and `Time`; CTTV's index row holds
the date and the meeting name. In both cases the name is known before the fetch, is better
than anything in the document, and does not depend on a URL segment that must not be trusted
for identity anyway.

**A snapshot must be checkable against a declared size.** — *2 sightings: OnBase Agenda
Online, CTTV captions*

The same defect from both ends. OnBase's search returns exactly 100 and declares nothing —
no total, no cursor, no marker — so a truncated snapshot is byte-identical to a complete one.
CTTV's grid prints **"2606 items in 53 pages"** in its footer, so the same truncation is
detectable for free.

`CONTEXT.md` already forbids a silent cap on a DiscoveryRun, because *a truncated snapshot
looks exactly like a source that shrank*. What these two add is that the rule was written
against **us** capping, and both of these are the **server** capping. The lever splits in
two: record a declared total and refuse a snapshot that misses it; and where no total is
declared, treat "exactly the page size" as suspect and narrow until every window comes back
short.

**Enumerate is not a GET.** — *2 sightings: OnBase Agenda Online, CTTV captions*

On both sites the list of addresses is reachable only by POST, with state read off a page
first. OnBase needs a session cookie and an ASP.NET `__RequestVerificationToken` scraped
from the search form. CTTV needs `__VIEWSTATE` carried forward from the previous response,
and offers no page parameter in any URL at all.

`SiteSource` enumerates by fetching a sitemap. Nothing in the current design can hold a
cookie jar, read a token out of a page, or carry opaque state from one request to the next —
so on both sites, discovery is not merely wrong, it is unexpressible. CTTV is the harder of
the two and shows where the seam has to sit: the state is opaque, so whatever performs
enumeration must be able to do a *sequence* of requests, not one.

Entry 15 meets the same wall one stage later. Accela Citizen Access lists its records by GET,
and serves every attached document only as the answer to a postback inside a session the
record page opened. So the sequence is wanted by acquire too, and the seam belongs in the
fetch layer that both stages share, not in either stage.

Fourth sighting, entry 20: Tampa's lobbyist meeting log, a RadGrid on CTTV's host. It narrows
the claim: the export POST needed no cookie, only the hidden fields of the GET before it, so
the sequence is two requests and the state is in the page. Entry 20 records the smallest
design that follows. OnBase has since moved its search to a GET (entry 7).

**The visible URL is not the fetchable one.** — *2 sightings: OpenGov Stories, OnBase Agenda
Online*

On both sites the address in the browser bar is a client-side route, and the address that
returns the document is a different one that a person never sees. OpenGov moves
`?currentPageId` and the server ignores it — 75 addresses, one document. OnBase's
`ViewMeeting?id=2500` is a 15 KB shell, and the minutes are at
`Documents/ViewAgenda?meetingId=2500&doctype=2`.

Consequence worth stating: **the URL a person copies out of the browser is not a usable
seed.** Both sites in this file were handed over as exactly that kind of URL, and on both,
collecting it verbatim yields a wrapper. Whatever gets built, a human-supplied entry URL has
to be treated as a hint about where to look, not as an address to fetch.

**Look for the sanctioned bulk path before building a crawler.** — *2 sightings: OnBase
Agenda Online, Hillsborough Clerk*

Twice now the hard-won path was not the best one, and the better one was already published.
OnBase: I read JavaScript to reach an HTML view that was 84% base64, while
`DownloadFileBytes` served a clean PDF from a meeting ID. Hillsborough: HOVER is a
search-only interface with nothing to enumerate, and two clicks away sit ~6 GB of daily bulk
files with a readme describing the record layout.

Both times the wrong path was the one a person clicks, and the right one was a plain URL a
person never sees. The working order this suggests: **check for a bulk or export path first,
then a browser network log, and only then read the page's JavaScript.** Entry 2 cost the most
effort and produced the least because that order was reversed.

Corollary worth its own line: **"no collection strategy" is a valid finding.** HOVER should
not be crawled — there is no Resource set behind a query box, and generating case numbers
would invent a corpus rather than observe one.

### Candidates, at one sighting each

- **A page that lists more than 25 documents loses the rest.** The enclosure cap is right for
  a wrapper and wrong for a listing, and it keeps the first 25 in page order — the oldest,
  when the list is chronological. (13: every Campus Suite file list, 39–262 a page — answered
  for that product by `campus-suite`; Wynnmere East 75, Newport Tampa Bay 144, Villages of
  Westport 102, THEA 32 on WordPress pages — **past two sightings**, and no product to key on.)
- **A TLS failure reaches the operator as "error sending request".** Two causes behind the
  one message: a chain to ISRG Root YR, which the compiled-in roots lack (13: ten Campus
  Suite districts), and a server offering only CBC suites (13: HART). The cause is in the
  error's source chain and is not printed.
- **robots.txt admits the pages and forbids the documents.** (13: CommunityXS at three GMS
  districts, `Disallow: /documents/*/download`; the `_assets` template at Water Street Tampa
  ID and Cory Lakes, `Disallow: /_assets/documents/`; SharePoint Online at Plan Hillsborough
  and TBRPC.) The ruling exists (`ignore_robots`); what is missing is a report that says a
  source's documents, as against its pages, were the part refused.

- **A CMS "external link" page redirects to a vendor, and the vendor's refusal is counted
  as the city's.** Two sites already (6: `dunedin.gov`, `myclearwater.com`), so this one is
  past a single sighting: stop at an off-host redirect and record it as a crumb.
- **`<base href>` decides what relative links mean, and nothing reads it.** 272 404s from
  one site, and the pages behind them never found. (6: `largo.com`)
- **One file can be most of a run's memory.** An 830 MB slideshow took the run from 63 MB to
  2.25 GB; a body must be refused before it is held, not after. (6: Hillsborough — fixed)
- **A real report bigger than the ceiling.** 464 MiB of flooding assessment; the ceiling is
  a memory guard, so what it refuses needs a way to be taken on purpose. (6: Hillsborough;
  19: Pinellas County's two six-year capital plans, 319 and 366 MB — **second sighting**)
- **A relative `Sitemap:` line is never resolved, and the site is total loss.** Legal, common,
  and it takes 579 addresses to zero while blaming another host. (5: `buffalony.gov`)
- **A capped walk over a single sitemap reports itself complete.** The cap warning needs a
  next iteration that a one-document queue never has. (5: `dunedin.gov` — 500 printed against
  1,625)
- **Boilerplate stripping can empty a document, and the empty one vanishes.** The strip is
  correct; the page had nothing else. Kin to *the pipeline measures and does not report*, but
  distinct — here the fault is created by a pass that is working. (5: `dunedin.gov` — 5 pages)
- **A chrome line shorter than the floor is never counted.** `Toggle Menu` is 11 characters
  against a 12-character minimum, and leads nearly every snippet on the site. (5: `boston.gov`)
- **A manual run leaves no record in `history`.** Only the scheduler writes the journal, so the
  discover-to-collect ratio survives in scrollback and nowhere else. (5: medina, dunedin — two
  sightings of the same missing write, one defect)
- **A shared enclosure is re-fetched once per referring page.** One `.docx` fetched 42 times in
  a 50-page run. Content addressing absorbs the storage; the requests are still made.
  (5: `dunedin.gov`; 6: `townofredingtonshores.com`, six footer PDFs fetched up to 1,026
  times each, 21.5 GB of repeats across ten sources — **second sighting**)
- **A plugin generates most of the site.** 7,841 of 9,939 addresses are auto-generated venue
  stubs, many near-duplicates. Changes what "collect the whole site" is worth.
  (5: `medinaco.org`; 6: `madeirabeachfl.gov`, Yoast attachment sitemaps, up to 2,000 of
  4,884 addresses are wrappers around uploads — **second sighting**; 6: `hillstaxfl.gov`,
  ten WooCommerce product sitemaps of student artwork at a 10 s crawl delay — **third**)
- **`application/octet-stream` defeats classification.** A `.csv` served with the one header
  that asserts nothing reaches `other`, and no reader claims it. (4: Hillsborough — 2.2 GB)
- **Both signals honest, both wrong about shape.** A `.txt` served `text/plain` whose first
  line is a 31-field pipe-delimited header. No header rule reaches it; only the content
  says. Rare, and the same finding as *a record set is not a document* from the other side.
  (4: Hillsborough)
- **A record set is not a document.** Fixed-width and delimited files extract "perfectly"
  into chunks that span dozens of unrelated records and embed to nothing. The row is already
  the unit. (4: Hillsborough — a 1.13 GB flat file of 174-character records)
- **An archive is a container of Artifacts, not a document.** `.zip` routes to `anydoc`,
  which expects one document; these hold many. Closer to acquisition than extraction.
  (4: Hillsborough — 30 ZIPs of sentencing guidelines)
- **Enumerate by walking an open directory listing.** No POST, no token, no cap, no
  JavaScript. (4: Hillsborough)
- **Extracted tables lose their cell and row boundaries.** `8/3/2026Tampa City Council
  Special Discussion` — fifty rows as one 10,736-character line. (3: CTTV). Not a site
  shape; a defect any table triggers, which is why it ranks first for a fix.
- **The publisher already produced the derived artifact.** A transcript published beside the
  video makes the `transcribe` stage redundant work for a worse result. (3: CTTV; 14:
  Granicus, a WebVTT track beside every player, captioned at four of seven clients —
  **second sighting**, and the one that built a reader)
- **A document link names no extension.** The enclosure scan keeps only paths with a
  reader's extension, so neither CivicPlus Document Center's
  `/DocumentCenter/View/7167/Lobbyists-Annual-Registration-2026` nor Liferay's
  `/documents/d/guest/{slug}?download=true` is ever a candidate, and eight box sources hold
  none of their files. (19: `hillsclerk.com`, seven ACFRs; 20: every CivicEngage source on
  the box, one product, eight sites — **two products**)
- **A server declares CSV as a spreadsheet.** `application/vnd.ms-excel` over plain text, so
  Excel opens it; believed, it is Underivable. (20: VoterFocus — fixed)
- **Errors answer HTTP 200.** A dead address becomes a live Observation and its error text
  is indexed. (2: OnBase; 20: VoterFocus, an unknown filer is 200 with an empty body and
  another filer's report an empty report; EFDMS, an unknown filing is 200 with an empty body
  — **second and third sightings**. Built from anchors, neither walk asks for one.)
- **Enclosure detection matches JavaScript source.** A URL template in a `<script>` becomes
  a fetch. (2: OnBase; 6: `tampa.gov`, a download widget's bare `fileName`, 196 × 404 a run
  — **second sighting**)
- **Extracted text carries base64 `data:` URIs.** (2: OnBase — 43% of one document). Not
  really a site shape at all; it is a defect any site can trigger, which is why it ranks
  first for a fix.
- **One document, two renderings, and the better one is not the one on screen.** OnBase
  renders one Word file as an HTML fragment (84% base64) and as a PDF (clean, more real
  text). Kin to the `tampa.gov` print-notice case, but not the same shape: there the HTML
  was a *wrapper* around a document held elsewhere; here both addresses hold the whole
  document and only the quality differs. Worth watching for a second true sighting.
- **An address with a decorative segment.** The path carries a filename the server ignores,
  so one document has unbounded addresses and Resource identity needs canonicalising.
  (2: OnBase)
- **Public to a browser, challenged for a client.** The government publishes the document
  without a login, and a bot challenge answers anything that is not a browser, robots.txt
  included. It is filed as Blocked, and that is correct; no lever. (16: OpenGov Procurement,
  six governments; Manatee's RFP Post — **second sighting**; PlanetBids answers `202` with an
  empty body, a third of the same kind)
- **A search hit carries no stable address.** Each run's results name documents by a token
  minted for that search, so the same document is a new Resource every run. (16: OnBase
  Public Access at the Hillsborough Clerk — 202 hits twice, 0 IDs in common)
- **The page is a wrapper and the content is one address away.** (`tampa.gov` PDF viewer
  pages — already solved by enclosures; recorded here so the next site of this shape is
  recognised rather than rediscovered)
- **A page lists more documents than a page may enclose, and nothing takes the rest.** The
  enclosure scan keeps 25 a page because a listing belongs to `enumerate`; `links` follows no
  document because the enclosure scan will. (19: Largo's document center, 165 PDFs, ACFRs at
  112–120; Pasco's budget office, 38, older adopted budgets at 27–33; Hillsborough's Clerk, 99
  — **three sightings**, all finance pages. A framework decision, proposed in entry 19)
- **A school district's files are in a script payload on another host.** Apptegy names each
  file on `files-backend.assets.thrillshare.com` only inside the page's Nuxt state. (19:
  Manatee and Hillsborough schools — two sightings, enumeration unmeasured)
- **The rules admit Centinel and the edge does not.** `robots.txt` allows `*`, and every page
  answers a Cloudflare managed challenge, "Just a moment" or "Attention Required", 403. (19:
  Municode, American Legal, eCode360, the Pinellas Clerk; 20: `votehillsborough.gov`,
  `votepinellas.gov`, `mypinellasclerk.gov`, the first two publishing their records on a
  vendor that answers)
