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
5. **Replace the readability floor with a retention ratio**, using the two yields the
   fallback path already measures.

Item 5 is the one that addresses fault 3, which is the largest by content lost, and it is the
one to be most careful about: `docs/STRATEGIES.md` §17 already records one read measure
withdrawn after the corpus contradicted it. A ratio has to be measured against these six
sites before it is trusted, not tuned until it looks right.

### Left on the table

- **A second sighting of "the extractor picked a different region than the page's own
  template implies".** All four sightings here are readability guessing. A site that marks its
  content region explicitly — `<main>`, `role="main"`, a schema.org `articleBody` — would say
  whether the fix is a better ratio or a better region test. None of these six was checked for
  that.
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
of test**: 378 chars is plausible for a short page and catastrophic for a 132 KiB one. The
comparison that separates them — readability's yield against the whole document's — is
already computed on the path that reports *"readability found only 123 chars"*.

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
  (5: `dunedin.gov`)
- **A plugin generates most of the site.** 7,841 of 9,939 addresses are auto-generated venue
  stubs, many near-duplicates. Changes what "collect the whole site" is worth.
  (5: `medinaco.org`)
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
  video makes the `transcribe` stage redundant work for a worse result. (3: CTTV)
- **Errors answer HTTP 200.** A dead address becomes a live Observation and its error text
  is indexed. (2: OnBase)
- **Enclosure detection matches JavaScript source.** A URL template in a `<script>` becomes
  a fetch. (2: OnBase)
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
- **The page is a wrapper and the content is one address away.** (`tampa.gov` PDF viewer
  pages — already solved by enclosures; recorded here so the next site of this shape is
  recognised rather than rediscovered)
