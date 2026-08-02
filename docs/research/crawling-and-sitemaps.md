# Crawling and Site Mapping — Primary-Source Research

**For:** Centinel v2 language decision (Rust vs Python vs TypeScript)
**Date of research:** 2026-08-02
**Scope:** crawling frameworks, 429 avoidance, robots.txt/sitemap parsing, HTML→markdown, JS rendering, Firecrawl, government CMS APIs.
**This document does not pick a language.** It records what each ecosystem actually has, with a link to the source that owns each claim.

Every version/date below was read from a registry API, a repository file, or an official doc page on 2026-08-02.

---

## 1. Firecrawl — deep dive

### 1.1 The headline finding: there IS an official Rust SDK

The operator's stated concern — *"it might just be ts and no rust support"* — **is not correct as of today.**

Firecrawl ships **ten** official SDKs, listed on their own SDK overview page: Python, Node.js/TypeScript, **Go**, Java, Ruby, **Rust**, .NET, PHP, Elixir, and a CLI.
Source: <https://docs.firecrawl.dev/sdks/overview>, <https://docs.firecrawl.dev/quickstarts/rust>

The Rust SDK is real, published, and current:

| Field | Value | Source |
|---|---|---|
| Crate | `firecrawl` | <https://crates.io/crates/firecrawl> |
| Description | "Official Rust SDK for Firecrawl API v2." | crates.io API `/api/v1/crates/firecrawl` |
| Latest version | **2.12.1**, published **2026-07-27** | crates.io API |
| First published | 2024-08-15 | crates.io API |
| License | **MIT** (`license = "MIT"` in `Cargo.toml`) | <https://github.com/firecrawl/firecrawl/blob/main/apps/rust-sdk/Cargo.toml> |
| Total downloads | 10,756 (2,018 in last 90 days) | crates.io API |
| Maintained in | Firecrawl monorepo, `apps/rust-sdk` | <https://github.com/firecrawl/firecrawl/tree/main/apps/rust-sdk> |
| Classification | **Pure Rust.** Deps: `reqwest`, `serde`, `serde_json`, `serde_with`, `thiserror`, `tokio`. No native/FFI deps, no shelling out. | `Cargo.toml` above |

**Release cadence is real, not abandonware.** crates.io shows 15+ releases between 2026-05-05 and 2026-07-27 (2.3.1 → 2.12.1). Commits touching `apps/rust-sdk` in the last ~7 weeks include `feat(sdks): add search highlights option (#4042)` (2026-07-16), `chore(sdks): bump go/php/rust/java/elixir for search monitor support` (2026-06-30), and `feat: add menu scrape format (engine client + 9 SDKs, menuBeta-gated) (#3831)` (2026-06-19).
Source: <https://github.com/firecrawl/firecrawl/commits/main/apps/rust-sdk>

**Caveat — the Rust SDK is a second-class citizen in cadence, not in existence.** Feature commits land in the JS/Python SDKs first and are then batch-ported to the "other 7" SDKs (see the literal commit message `chore(sdks): bump go/php/rust/java/elixir for ...`). Version numbers diverge accordingly:

| SDK | Latest | Released | Source |
|---|---|---|---|
| `firecrawl-py` (PyPI) | 4.34.0 | 2026-07-31 | <https://pypi.org/pypi/firecrawl-py/json> |
| `@mendable/firecrawl-js` / `firecrawl` (npm) | 4.32.0 | 2026-07-31 | <https://registry.npmjs.org/firecrawl> |
| `firecrawl` (crates.io) | 2.12.1 | 2026-07-27 | <https://crates.io/crates/firecrawl> |

**Rust SDK surface area** (source files in `apps/rust-sdk/src`): `scrape.rs`, `crawl.rs`, `map.rs`, `search.rs`, `batch_scrape.rs`, `parse.rs`, `agent.rs`, `monitor.rs`, `research.rs`, `client.rs`, `types.rs`.
Source: <https://github.com/firecrawl/firecrawl/tree/main/apps/rust-sdk/src>

Notably **absent from the Rust SDK: `extract.rs`** — the `/extract` endpoint exists in the API (see §1.3) but has no dedicated Rust module, and there is no `interact.rs` for the standalone browser-session endpoints. Everything Centinel actually needs (scrape, crawl, map, search, batch, parse) is present.

The Rust SDK also carries a **near-empty CHANGELOG** — it has exactly two entries, `[2.5.0]` and `[0.1]` — so changelog-driven upgrade review is not available for the Rust SDK the way it is for JS.
Source: <https://github.com/firecrawl/firecrawl/blob/main/apps/rust-sdk/CHANGELOG.md>

### 1.2 The bigger finding: Firecrawl's own crawl core is written in Rust

This matters more than the SDK question for a language decision.

Firecrawl is a TypeScript product (`"language": "TypeScript"` on the repo). But the parts of it that do the work this project cares about — link filtering, robots.txt evaluation, HTML transformation, XML/sitemap parsing, PDF and Office document parsing — were moved into a **Rust native module** compiled with `napi-rs` and loaded into Node.

Crate `firecrawl_rs` at `apps/api/native/`:
- `src/crawler.rs` (30 KB) — link filtering, depth limits, include/exclude regex, subdomain rules, **and robots.txt allow/deny** via `use texting_robots::Robot;`
- `src/html.rs` (28 KB), `src/pdf.rs`, `src/document/` (Office/OLE/XML renderers)
- Dependencies chosen: `texting_robots 0.2.2` (robots.txt), `lol_html 2.6.0` + `kuchikiki 0.8.2` (HTML rewriting/DOM), `roxmltree 0.20` (XML/sitemaps), `psl 2.1` (public suffix), `calamine` (spreadsheets), `pdf-inspector`, `zip`, `encoding_rs`

Sources:
- <https://github.com/firecrawl/firecrawl/blob/main/apps/api/native/Cargo.toml>
- <https://github.com/firecrawl/firecrawl/blob/main/apps/api/native/src/crawler.rs>
- <https://github.com/firecrawl/firecrawl/tree/main/apps/api/native/src>

**Read that as evidence in the language decision:** the leading commercial crawling product reached for the Rust crate ecosystem — specifically `texting_robots`, `lol_html`, `roxmltree` — to build exactly the primitives Centinel needs. Those crates are load-bearing in production at Firecrawl's scale.

### 1.3 What Firecrawl actually offers today

Read directly from the v2 OpenAPI spec (`https://docs.firecrawl.dev/api-reference/v2-openapi.json`, `info.version: v2`, server `https://api.firecrawl.dev/v2`). Endpoints relevant here:

| Endpoint | Methods | What it does |
|---|---|---|
| `POST /scrape` | POST | Single URL → markdown / html / rawHtml / links / images / screenshot / json / summary / **changeTracking** |
| `POST /crawl`, `GET /crawl/{id}`, `GET /crawl/{id}/errors`, `DELETE /crawl/{id}`, `GET /crawl/active`, `POST /crawl/params-preview` | mixed | Async recursive site crawl with job polling |
| `POST /map` | POST | URL discovery for a domain |
| `POST /search` | POST | Web search, optionally scraping each result |
| `POST /extract`, `GET /extract/{id}` | POST/GET | LLM structured extraction across pages |
| `POST /batch/scrape` + status/errors | mixed | Bulk scrape of a known URL list |
| `POST /parse` | POST | **Upload a local/non-public file** (pdf, docx, xlsx…) and get markdown back |
| `POST /monitor` + `/monitor/{id}/run`, `/checks` | mixed | Scheduled recurring re-crawl with change detection |
| `POST /agent`, `/interact`, `/scrape/{jobId}/interact` | mixed | LLM-driven browsing and live browser sessions |

Full path list confirmed from the OpenAPI JSON above.

**`/scrape` defaults that matter** (from `components.schemas.ScrapeOptions` in the OpenAPI spec):

| Option | Default | Doc text (verbatim, abridged) |
|---|---|---|
| `formats` | `["markdown"]` | — |
| `onlyMainContent` | `true` | *"Only return the main content of the page excluding headers, navs, footers, etc. This is a **deterministic HTML-level filter** applied before markdown is generated; **no LLM is involved**."* |
| `parsers` | `["pdf"]` | *"When `"pdf"` is included (default), the PDF content is extracted and converted to markdown format, with billing based on the number of pages (1 credit per page). When an empty array is passed, the PDF file is returned in base64…"* |
| `maxAge` | `172800000` (2 days) | Returns cached page if younger than this. |
| `proxy` | `auto` | `basic` / `enhanced` (up to 5 credits) / `auto`. |
| `blockAds` | `true` | Ad and cookie-popup blocking. |
| `timeout` | `60000` ms | min 1000, max 300000. |
| `waitFor` | `0` | Extra delay before capture, on top of "smart wait". |

That `onlyMainContent` line is important: **Firecrawl's boilerplate stripping is deterministic DOM filtering, not an LLM pass.** It is reproducible and free.

### 1.4 Does `/map` produce a full site map, or a sample?

**It is explicitly a fast approximation, and Firecrawl says so.** From the feature docs: *"This endpoint prioritizes speed, so it may not capture all website links,"* and they recommend `/crawl` for comprehensive discovery.
Source: <https://docs.firecrawl.dev/features/map>

Discovery strategy, verbatim: *"URLs are primarily discovered from the website's sitemap, supplemented with SERP (search engine) results and previously crawled pages to improve coverage."* — i.e. it blends the site's own sitemap with **Firecrawl's index of prior crawls and search results**, which is a source of URLs you cannot reproduce yourself.

`/map` parameters (from the OpenAPI spec):

| Param | Default | Max | Notes |
|---|---|---|---|
| `limit` | **5000** | **100000** | "Maximum number of links to return" |
| `sitemap` | `include` | — | `skip` = don't use sitemap; `only` = **sitemap URLs only**; `include` = sitemap + other methods |
| `includeSubdomains` | `true` | — | |
| `ignoreQueryParameters` | `true` | — | "Do not return URLs with query parameters" |
| `ignoreCache` | `false` | — | "Sitemap data is cached for up to **7 days**" |
| `search` | — | — | Rank/filter returned URLs by relevance to a term |
| `timeout` | none | — | "There is no timeout by default" |

**Verdict for a site-map-as-file-tree:** `/map` is a good *seed* and a good *discovery-delta detector* (cheap: 1 credit per call regardless of URL count, per <https://www.firecrawl.dev/pricing>). It is **not** a guaranteed-complete enumeration. For "every page, every version" you need `/crawl` (default `limit: 10000`, per the OpenAPI spec) or your own crawler. Note also `ignoreQueryParameters: true` by default will silently drop query-string URLs — and a lot of `.gov` agenda/document systems are query-string addressed (see §7).

### 1.5 Output formats and markdown quality

Firecrawl's HTML→markdown pipeline, read from source, is a three-tier fallback:

1. **Preferred:** an HTTP microservice (`HTML_TO_MARKDOWN_SERVICE_URL`) — `apps/go-html-to-md-service`, a Go service built on `github.com/firecrawl/html-to-markdown` (a Firecrawl fork) + `PuerkitoBio/goquery`.
2. **Then:** the same Go converter loaded as a **shared library over `koffi` FFI** (`USE_GO_MARKDOWN_PARSER`).
3. **Fallback:** `turndown` ^7.1.3 + `joplin-turndown-plugin-gfm` in pure JS.

Every path then runs `postProcessMarkdown` from `@mendable/firecrawl-rs` — the **Rust** native module.

Sources:
- <https://github.com/firecrawl/firecrawl/blob/main/apps/api/src/lib/html-to-markdown.ts>
- <https://github.com/firecrawl/firecrawl/blob/main/apps/go-html-to-md-service/go.mod>
- <https://github.com/firecrawl/firecrawl/blob/main/apps/api/package.json> (`turndown`, `joplin-turndown-plugin-gfm`, `koffi`, `cheerio`, `jsdom`, `robots-parser`, `pdf-parse`)

**Live quality check.** I scraped a real `.gov` page keyless on 2026-08-02:

```
POST https://api.firecrawl.dev/v2/scrape   {"url":"https://www.tampa.gov/city-council"}
→ HTTP 200, success: true, markdown length 9,088 chars
```

Observed: clean GFM, headings preserved, **all links and image srcs rewritten to absolute URLs**, rich metadata harvested (`title`, `ogTitle`, `ogDescription`, `modifiedTime: 2026-07-29T09:42:08-04:00`, `language: en`, og/twitter tags). `onlyMainContent: true` removed site chrome, though a `[Skip to main content](...)` accessibility link survived at the top — boilerplate stripping is good, not perfect.

That `modifiedTime` in metadata is directly useful for Centinel's change-delta tracking.

### 1.6 Change tracking — unexpectedly on-mission

Firecrawl has a `changeTracking` scrape format that is close to what Centinel is building:

- `changeStatus`: `"new"` | `"same"` | `"changed"` | `"removed"`
- `previousScrapeAt`: timestamp of last comparison
- `visibility`: `"visible"` (reachable via links/sitemap) vs **`"hidden"`** (URL still 200s but is no longer linked from anywhere)
- `modes: ["git-diff"]` → line-level diff, returned both as plain-text diff and as structured JSON with additions/deletions/line numbers
- `modes: ["json"]` → field-level schema comparison, `previous` vs `current` per field (costs 5 credits/page; plain tracking is free)
- **Requirement:** *"The markdown format is required because change tracking compares pages via their markdown content."*

Source: <https://docs.firecrawl.dev/features/change-tracking>, and the `changeTracking` entry in `components.schemas.Formats` of the OpenAPI spec.

There is also a whole `/monitor` endpoint family for scheduled recurring crawls with change detection and webhook/email alerts (<https://docs.firecrawl.dev/features/monitoring>).

The `visibility: hidden` signal in particular is hard to build yourself and is exactly a "discovery delta."

### 1.7 JS-rendered pages

- **Cloud:** handled by "Fire-engine," which selects among engines including `fire-engine;chrome-cdp`, `fire-engine;tlsclient`, and stealth variants. Engine names visible in <https://github.com/firecrawl/firecrawl/blob/main/apps/api/src/lib/robots-txt.ts>.
- **Self-hosted:** a `playwright-service` container (`apps/playwright-service-ts`, `PLAYWRIGHT_MICROSERVICE_URL`, `MAX_CONCURRENT_PAGES`) plus plain `fetch`. Source: <https://github.com/firecrawl/firecrawl/blob/main/docker-compose.yaml>
- Page interaction is available via `actions` on `/scrape`, and via the newer `/interact` browser-session endpoints (cloud only).

### 1.8 PDFs and documents — handled, not skipped

`parsers` defaults to `["pdf"]`; PDFs are extracted and converted to markdown at **1 credit per page**. Modes: `auto` (fast text extraction, OCR fallback), `fast` (text only, skips scanned pages), `ocr` (force OCR every page). `maxPages` is supported: `parsers: [{ type: "pdf", mode: "ocr", maxPages: 20 }]`.

Documented document types: **PDF** (`.pdf`), **Word** (`.docx`, `.doc`, `.odt`, `.rtf`), **Excel** (`.xlsx`, `.xls`). PPTX is *not* listed.
Source: <https://docs.firecrawl.dev/features/document-parsing>

`POST /parse` additionally accepts a **direct file upload** (multipart) for local or non-public documents — useful when you've already harvested a PDF and just want markdown out of it, without Firecrawl re-fetching it.
Source: <https://docs.firecrawl.dev/features/parse>, and `apps/rust-sdk/README.md` shows `ParseFile::from_bytes(...)` working in Rust.

### 1.9 robots.txt posture

**Firecrawl respects robots.txt by default, and turning that off is gated behind an enterprise contract.** From the OpenAPI `/crawl` schema:

- `ignoreRobotsTxt`: default `false` — *"Ignore the website's robots.txt rules. **Enterprise only — contact support@firecrawl.com to enable.**"*
- `robotsUserAgent`: *"Custom User-Agent string for robots.txt evaluation… **Enterprise only**."*

README, verbatim: *"**It is the sole responsibility of end users to respect websites' policies when scraping.** … **By default, Firecrawl respects robots.txt directives.** By using Firecrawl, you agree to comply with these conditions."*
Source: <https://github.com/firecrawl/firecrawl/blob/main/README.md#license>

robots.txt is fetched with a 24-hour cache (`ROBOTS_MAX_AGE = 1 * 24 * 60 * 60 * 1000`) and evaluated with npm `robots-parser` ^3.0.1 on the TS side / `texting_robots` on the Rust side.
Source: <https://github.com/firecrawl/firecrawl/blob/main/apps/api/src/lib/robots-txt.ts>

Politeness controls on `/crawl`: `delay` (seconds between scrapes — *"Setting this forces concurrency to 1"*) and `maxConcurrency`. Both from the OpenAPI spec.

### 1.10 Hosted vs self-hosted

**Repo license: AGPL-3.0** (<https://github.com/firecrawl/firecrawl> → `license.spdx_id: "AGPL-3.0"`). README: *"This project is primarily licensed under the GNU Affero General Public License v3.0 (AGPL-3.0). The SDKs and some UI components are licensed under the MIT License."* Verified: `apps/js-sdk`, `apps/python-sdk`, `apps/ruby-sdk`, `apps/elixir-sdk` carry their own `LICENSE`; `rust-sdk`, `go-sdk`, `java-sdk`, `php-sdk`, `dot-net-sdk` do not ship a LICENSE file but declare MIT in their manifests (`Cargo.toml` for Rust).

**AGPL is the real self-host consideration**, not the SDK question. Centinel consuming the *SDK* (MIT) over the *hosted API* has no copyleft exposure. Self-hosting the AGPL server and exposing it over a network is where AGPL §13 applies.

**What self-hosting requires** (`docker-compose.yaml`): `api`, `playwright-service`, `redis`, `rabbitmq`, `nuq-postgres` (their Postgres-backed queue), plus optional `foundationdb` + `foundationdb-init` (`NUQ_BACKEND=fdb`). Env includes `NUM_WORKERS_PER_QUEUE` (8), `CRAWL_CONCURRENT_REQUESTS` (10), `MAX_CONCURRENT_JOBS` (5), `BROWSER_POOL_SIZE` (5).
Source: <https://github.com/firecrawl/firecrawl/blob/main/docker-compose.yaml>

**What self-hosting loses.** The self-host guide states verbatim: *"Currently, self-hosted instances of Firecrawl do not have access to **Fire-engine**, which includes advanced features for handling IP blocks, robot detection mechanisms, and more."* Also missing/unsupported without extra configuration per the same guide: `/agent` and browser endpoints, advanced proxy management, JSON format on scrape, `/extract`, summary/branding formats, and **change tracking**. And: *"Right now it's not possible to configure Supabase in self-hosted instances."*
Sources: <https://github.com/firecrawl/firecrawl/blob/main/SELF_HOST.md>, <https://docs.firecrawl.dev/contributing/self-host>

**Blunt read:** self-hosted Firecrawl is a scrape/crawl/map engine with Playwright and markdown conversion. The differentiated features — the URL index that makes `/map` good, change tracking, anti-bot — are cloud-only. For `.gov` targets, anti-bot matters much less than it would for e-commerce, but change tracking is precisely the feature Centinel wants, and self-hosting does not give it to you.

### 1.11 Rate limits and pricing (hosted)

Requests per minute, from <https://docs.firecrawl.dev/rate-limits>:

| Plan | `/scrape` | `/map` | `/crawl` | `/search` | `/crawl/status` | Concurrent browsers |
|---|---|---|---|---|---|---|
| Free | 10 | 10 | 2 | 10 | 500 | 2 |
| Hobby | 100 | 100 | 20 | 100 | 5,000 | 5 |
| Standard | 500 | 500 | 100 | 500 | 25,000 | 50 |
| Growth | 5,000 | 5,000 | 1,000 | 5,000 | 250,000 | 100 |
| Scale | 10,000 | 10,000 | 2,000 | 10,000 | 500,000 | 150+ |

Max queued jobs: 50,000 (Free/Hobby) → 300,000+ (Scale). Jobs queued >48h expire.

Pricing, from <https://www.firecrawl.dev/pricing>:

| Plan | Monthly | Credits/mo | Concurrent |
|---|---|---|---|
| Free | $0 | 1,000 (+1,000 search) | 2 |
| Hobby | $16 | 5,000 | 5 |
| Standard | $83 | 100,000 | 50 |
| Growth | $333 | 500,000 | 100 |
| Scale | $599 | 1,000,000 | 150 |
| Enterprise | custom | custom | custom |

Credit costs: scrape/crawl/map/monitor = **1 credit per page**; search = 2 credits/10 results; PDF = 1 credit **per page**; `enhanced` proxy = up to 5 credits/request; changeTracking `json` mode = 5 credits/page. *"Credits do not roll over on self-serve plans."* No pay-per-use option.

**Cost model implication for Centinel:** "every version of every page, retained forever" means re-scraping the whole corpus on each pass. A 50,000-page corpus re-crawled weekly is ~2.6M credits/year → Scale tier or beyond, before PDFs (which bill per page and will dominate — a 300-page budget PDF is 300 credits). This is the number that should decide hosted-vs-self-vs-own-crawler, not the SDK question.

**Keyless tier exists but is crippled.** Verified live on 2026-08-02:
- `POST /v2/scrape` with no auth → **HTTP 200**, full markdown returned.
- `POST /v2/map` with no auth → **HTTP 401**: `"This endpoint is not supported by the keyless free tier. Sign up for a free API key…"`

Docs confirm keyless is *"capped per IP address per day by two limits"* (max requests and max credits).

### 1.12 If there were no Rust SDK, how bad would it be?

Moot — there is one. But the counterfactual is worth stating because it generalizes:

The Firecrawl Rust SDK's entire dependency set is `reqwest + serde + serde_json + serde_with + thiserror + tokio`. It is a typed JSON-over-HTTP client and nothing more. Firecrawl publishes a **complete OpenAPI 3.0 spec** (<https://docs.firecrawl.dev/api-reference/v2-openapi.json>, 328 KB), from which a client can be generated in any language.

**"No SDK in language X" is not load-bearing for Firecrawl.** The API is plain JSON over HTTPS with a bearer token; async jobs are `POST` + poll `GET /crawl/{id}`; there are also webhooks so you don't have to poll. Writing that client by hand in Rust is roughly a day of work and gives you exact control over retry and backoff behavior, which a vendor SDK would hide.

The *real* asymmetries between languages here are elsewhere: the LangChain/LlamaIndex/Vercel-AI-SDK integrations are Python/TS-only, the MCP server is TS, and the community/example surface is Python/TS-first (see the quickstart list in <https://docs.firecrawl.dev/llms.txt> — Django, FastAPI, Flask, Next.js, Nuxt, SvelteKit, Express… versus a single generic Rust quickstart).

---

## 2. Ground truth: what `.gov` surfaces actually do

Everything in this section was measured live on 2026-08-02 with `curl` against production `.gov` hosts. It is included because it changes which library features actually matter.

### 2.1 The real blocking mode is 403 from a CDN/WAF, not 429 with `Retry-After`

| Host | Default `curl` UA | Browser / declared UA | Server header |
|---|---|---|---|
| `www.phila.gov/robots.txt` | **HTTP 403**, `Request blocked.` | **HTTP 200**, full page | `server: CloudFront`, `x-cache: Error from cloudfront` |
| `www.sec.gov/robots.txt` | **HTTP 403**, HTML titled *"SEC.gov \| Request Rate Threshold Exceeded"* | **HTTP 200**, real robots.txt | `server: AkamaiGHost` |

**Neither 403 response carried a `Retry-After` header.** Full header dump of the SEC block: `server: AkamaiGHost`, `mime-version`, `content-length: 1925`, `cache-control: no-cache…`, `pragma`, `expires`, `content-type: text/html`, `date`, `strict-transport-security`. Nothing else. CloudFront's was `server: CloudFront`, `x-cache`, `via`, `x-amz-cf-pop`, `x-amz-cf-id`.

Implication: a retry policy keyed on `429` + `Retry-After` **will not fire** in the most common real-world `.gov` failure. What actually fixes it is a descriptive User-Agent. Which leads to:

### 2.2 Some `.gov` operators publish an explicit machine-access contract

SEC.gov, verbatim from <https://www.sec.gov/search-filings/edgar-search-assistance/accessing-edgar-data>:

> **Fair access — Current max request rate: 10 requests/second.** To ensure everyone has equitable access to SEC EDGAR content, please use efficient scripting. Download only what you need and please moderate requests to minimize server load. SEC reserves the right to limit request rates to preserve fair access for all users.

> The SEC does not allow botnets or automated tools to crawl the site. … **Please declare your user agent in request headers:**
> ```
> User-Agent: Sample Company Name AdminContact@<sample company domain>.com
> Accept-Encoding: gzip, deflate
> Host: www.sec.gov
> ```

Reproduced: `curl -A "Centinel Research ben@..."` flipped `www.sec.gov/robots.txt` from 403 → 200.

**A per-host policy table (UA string, req/sec cap, contact email) is a first-class requirement for this project, and no crawling library gives it to you.** It is application-layer config in every language.

### 2.3 `Crawl-delay` is rare; robots.txt shape is inconsistent

Survey of federal robots.txt:

| Host | Lines | `Crawl-delay` | `Sitemap:` |
|---|---|---|---|
| `www.nasa.gov` | 6 | **`Crawl-delay: 1`** | 0 |
| `www.cdc.gov` | 58 | 0 | 1 (`https://www.cdc.gov/wcms-auto-sitemap-index.xml`) |
| `www.irs.gov` | 109 | 0 | 1 |
| `www.sec.gov` | 33 | 0 | 0 |
| `www.epa.gov` | 78 | 0 | 0 |
| `www.energy.gov` | 72 | 0 | 0 |
| `www.usgs.gov` | 20 | 0 | 0 |
| `www.hhs.gov` | 10 | 0 | 0 |

So `Crawl-delay` support in a robots parser is nice-to-have, **not** the main lever. Wildcard support *is* required: `www.tampa.gov` (Drupal) uses `$`-anchored and `?`-suffixed patterns like `Allow: /core/*.css$` and `Allow: /core/*.js?`.

Municipal results were worse:
- `www.stpete.org/robots.txt` → **HTTP 404** (no robots.txt at all)
- `www.miamidade.gov/robots.txt` → returns an **HTML error page**, not `text/plain`. A parser that doesn't content-type-check will happily parse HTML as robots directives.

### 2.4 Sitemaps are almost never the textbook shape

- `www.tampa.gov/sitemap.xml` is a **`<sitemapindex>`** (Drupal `simple_sitemap`) with **6 children**, and the children are **query-string URLs**: `https://www.tampa.gov/sitemap.xml?page=1` … `?page=6`. Each child holds **2,000 `<url>` entries** → ~12,000 URLs total. It also carries an `<?xml-stylesheet?>` PI before the root element.
- `www.hillsboroughcounty.org/robots.txt` declares `Sitemap: https://hcfl.gov/sitemap` — a **different hostname** and **no `.xml` extension**. `HEAD` returns `HTTP 200, content-type: application/xml`.
- `www.cdc.gov` points at `wcms-auto-sitemap-index.xml`.

Requirements this imposes on a sitemap parser, in any language:
1. Handle `<sitemapindex>` recursively (index → index → urlset is legal).
2. Do not assume `.xml`/`.xml.gz` file extensions — sniff `content-type` and magic bytes.
3. Tolerate query-string `<loc>` values (so a "strip query params" normalizer must not run before sitemap fetching).
4. Tolerate cross-host `Sitemap:` directives.
5. Handle gzip (`.xml.gz`) transparently.
6. Tolerate XML stylesheet PIs, BOMs, and non-UTF-8 encodings.

---

## 3. Per-language crawling libraries

All versions, dates, licenses and download counts below were read on **2026-08-02** from the crates.io API (`https://crates.io/api/v1/crates/{name}`), the PyPI JSON API (`https://pypi.org/pypi/{name}/json`), the npm registry (`https://registry.npmjs.org/{name}`), and the GitHub repos API (`https://api.github.com/repos/{owner}/{repo}` for `pushed_at` / `archived`).

### 3.1 Rust

| Crate | Version | Released | License | `pushed_at` | Stars | Recent DL (90d) | Classification |
|---|---|---|---|---|---|---|---|
| [`spider`](https://crates.io/crates/spider) | 2.53.4 | 2026-07-30 | MIT | 2026-08-01 | 2,636 | 65,948 | Pure Rust framework |
| [`reqwest`](https://crates.io/crates/reqwest) | 0.13.4 | 2026-05-25 | MIT/Apache-2.0 | — | — | 155M | Pure Rust HTTP client |
| [`scraper`](https://crates.io/crates/scraper) | 0.27.0 | 2026-05-11 | **ISC** | — | — | 7.0M | Pure Rust (`html5ever` + `selectors`) |
| [`voyager`](https://crates.io/crates/voyager) | 0.2.1 | **2022-01-12** | MIT/Apache-2.0 | **2024-12-14** | 767 | **381** | Pure Rust — **effectively dead** |
| [`spider_chrome`](https://crates.io/crates/spider_chrome) | 2.37.130 | 2026-03-25 | MIT/Apache-2.0 | — | — | 109,695 | Pure Rust CDP client (fork of chromiumoxide) |

**`spider` is the only serious general-purpose crawling *framework* in Rust.** Everything else in the list is a component you assemble yourself.

- **Repo:** <https://github.com/spider-rs/spider>. Workspace members: `spider`, `spider_cli`, `spider_worker`, `spider_transformations`, `spider_utils`, `spider_chrome`, `spider_agent`.
- **Maintenance is unusually hot**, not merely alive: 2.53.4 published 2026-07-30, repo pushed 2026-08-01, **0 open issues**.
- **Feature-flag heavy.** Default feature set (`spider/Cargo.toml`, `[features]`) is `["basic", "io_uring", "tcp_fastopen", "splice", "numa", "zero_copy"]`, and `basic` expands to `__basic` which pulls in `sync, cookies, ua_generator, encoding, balance, real_browser, disk_native_tls, time, adaptive_concurrency, priority_frontier, dns_cache, rate_limit, request_coalesce, auto_throttle, etag_cache, warc`. Source: <https://github.com/spider-rs/spider/blob/main/spider/Cargo.toml>
- **JS rendering is a separate opt-in feature** (`chrome`, `chrome_headless`, backed by `spider_chrome`) — see §7.
- **`sitemap` is a feature flag** (`sitemap = ["dep:sitemap"]`) that pulls in the `sitemap` crate — a crate last released **2020-11-03** (see §5.1). That is spider's weakest dependency for this project's purposes.
- `spider_transformations` provides HTML→markdown/text/XML conversion in-tree (see §6.1).

**`voyager` should be treated as abandoned for a new project.** Last release 2022-01-12; last repo push 2024-12-14; **381 downloads in 90 days**. It is a thin `reqwest` + `scraper` + `futures` scheduler with a typed `Scraper` trait. Nothing in it is worth the maintenance risk over rolling the same loop yourself.
Source: <https://github.com/mattsse/voyager>

**`reqwest` + `scraper` is the realistic non-framework path**, and it is genuinely fine — but note it is *only* a fetcher and a CSS-selector DOM. Frontier, dedup, politeness, robots, sitemaps, retry, markdown conversion, and persistence are all yours to write. Note `scraper` is **ISC**, not the usual MIT/Apache dual — a licence a legal review may flag as unfamiliar even though it is permissive and OSI-approved.

Supporting crates you would end up adding on the assemble-it-yourself path (all pure Rust): `url`, `governor` 0.10.4 (2025-12-16, MIT, 13.2M recent DL — token-bucket rate limiting), `reqwest-middleware` 0.5.2 / `reqwest-retry` 0.9.1 (2026-02-05, MIT/Apache), `tower` (concurrency limits), `texting_robots` or `robotstxt` (§5.1), `roxmltree`/`quick-xml` (§5.1), `htmd` (§6.1).

### 3.2 Python

| Package | Version | Released | License | `pushed_at` | Stars | Classification |
|---|---|---|---|---|---|---|
| [`Scrapy`](https://pypi.org/project/Scrapy/) | 2.17.0 | 2026-07-07 | BSD-3-Clause | 2026-08-01 | 63,548 | Pure Python (on Twisted) |
| [`crawlee`](https://pypi.org/project/crawlee/) | 1.8.3 | 2026-07-20 | Apache-2.0 | 2026-07-30 | 9,392 | Pure Python (asyncio) |
| [`httpx`](https://pypi.org/project/httpx/) | 0.28.1 | **2024-12-06** | BSD-3-Clause | — | — | Pure Python |
| [`selectolax`](https://pypi.org/project/selectolax/) | 0.4.11 | 2026-07-15 | MIT | 2026-07-15 | 1,661 | **Cython binding to C** (`lexbor`/`modest`) |
| [`trafilatura`](https://pypi.org/project/trafilatura/) | 2.2.0 | 2026-07-31 | Apache-2.0 | 2026-07-31 | 6,386 | Pure Python (on `lxml`, a C binding) |

**Scrapy is the most complete crawling framework in any of the three languages, full stop.** It ships, in the box and enabled or one-setting-away: a scheduler with disk-backed request queues, duplicate filtering, `robots.txt` obedience (`ROBOTSTXT_OBEY`, parser pluggable via `ROBOTSTXT_PARSER`, default `ProtegoRobotParser`), retry middleware, redirect handling, HTTP caching, an offsite filter, sitemap spiders, feed exports, per-domain concurrency slots, and **AutoThrottle** (§4.1). Default settings verified in <https://github.com/scrapy/scrapy/blob/master/scrapy/settings/default_settings.py>.

Two structural caveats:
1. **Twisted, not asyncio.** Scrapy runs on the Twisted reactor. Modern Scrapy can install an asyncio reactor, but the framework's idiom is still callbacks + generators, and this is the single biggest ergonomic cost of choosing it.
2. **`ROBOTSTXT_OBEY = False` by default.** You must turn it on. (Same default posture as most tooling; Firecrawl's default is the opposite, §1.9.)

**`crawlee` (Python) is the modern asyncio alternative** and, for this project's specific requirements, it has one feature Scrapy does not: a real per-domain `Retry-After`-aware throttle (§4.2). It is Apify's port of the JS Crawlee and is *ahead of the JS original* on that specific axis. 1.8.3 released 2026-07-20.

**`httpx` is worth a warning flag.** Latest release **0.28.1, 2024-12-06** — over 19 months stale at time of writing, and still pre-1.0. It works, it is widely deployed, but "actively maintained" is a stretch. If you assemble your own crawler in Python, that is the fetcher you would reach for by default, and it is the least-maintained component in the stack.

**`selectolax` is the speed play and it is a binding, not pure Python.** It is a Cython wrapper over the C `lexbor` and `modest` HTML engines. Wheels are published, so it is not a build-from-source burden in practice, but it is a native extension: it constrains free-threaded/PyPy targets and adds a compiled artifact to any packaging story.

**`trafilatura` is not a crawler, it is a boilerplate-removal + extraction library** (see §6.2). It does ship a `spider` module for focused crawling and its own `courlan` URL-filtering package (1.4.0, 2026-06-01, Apache-2.0), but you would not build Centinel's crawl frontier on it. Its value is in §6.

### 3.3 TypeScript

| Package | Version | Released | License | `pushed_at` | Stars | Classification |
|---|---|---|---|---|---|---|
| [`crawlee`](https://www.npmjs.com/package/crawlee) | 3.17.0 | 2026-06-04 | Apache-2.0 | 2026-08-01 | 25,159 | Pure TS |
| [`playwright`](https://www.npmjs.com/package/playwright) | 1.62.1 | 2026-07-30 | Apache-2.0 | — | — | TS + **downloaded browser binaries** |
| [`puppeteer`](https://www.npmjs.com/package/puppeteer) | 25.4.0 | 2026-07-27 | Apache-2.0 | — | — | TS + **downloaded browser binaries** |
| [`cheerio`](https://www.npmjs.com/package/cheerio) | 1.2.0 | 2026-01-23 | MIT | — | — | Pure JS (`parse5`/`htmlparser2`) |
| [`got-scraping`](https://www.npmjs.com/package/got-scraping) | 4.2.1 | 2026-02-24 | Apache-2.0 | — | — | Pure JS (`got` + header/TLS fingerprint spoofing) |

**Crawlee is the TS answer to Scrapy** and is architecturally the closest thing in this survey to what Centinel needs: `BasicCrawler` → `HttpCrawler` / `CheerioCrawler` / `JSDOMCrawler` / `LinkeDOMCrawler` → `PlaywrightCrawler` / `PuppeteerCrawler`, all sharing one `RequestQueue`, one `AutoscaledPool`, one `SessionPool`, and one storage abstraction. Swapping HTTP-only for browser rendering is a class-name change, not a rewrite. Repo: <https://github.com/apify/crawlee>.

Crawlee-specific notes that matter here:
- It has first-class `enqueueLinks` with **`strategy: 'same-domain' | 'same-hostname' | 'same-origin' | 'all'`** and glob/regex filters — the exact scoping primitive a `.gov` crawl needs.
- It has built-in **sitemap** support (`Sitemap.load()`, `enqueueLinks({ strategy, ... })`, `RobotsTxtFile`) — see §5.3.
- `maxRequestsPerMinute` is a first-class crawler option (`packages/basic-crawler/src/internals/basic-crawler.ts`, line ~318) that maps to `AutoscaledPool.maxTasksPerMinute`.
- It is **Apify-platform-shaped**. The storage client, the `clientInfo` autoscaling signal, and the proxy configuration all assume Apify as the reference deployment. It works standalone (`MemoryStorage` / filesystem), but you inherit concepts you do not need.

**`cheerio` is the boring correct choice for DOM work in TS** and is fine. Note the long release gap pattern: 1.2.0 on 2026-01-23 with nothing since; cheerio's history is bursty, not dead.

**`got-scraping` is the TS-only capability with no Rust or Python equivalent of the same maturity.** It is `got` plus browser-realistic header ordering and TLS/JA3 fingerprint mimicry, maintained by Apify. Relevant to §4.5.

### 3.4 Cross-language summary

| Capability | Rust | Python | TypeScript |
|---|---|---|---|
| Batteries-included crawl framework | `spider` (1 option, very active) | **Scrapy** (mature) + `crawlee` (modern) | **`crawlee`** (mature) |
| Assemble-it-yourself HTTP + DOM | `reqwest` + `scraper` — excellent | `httpx` (stale) + `selectolax` (native ext) | `undici`/`got` + `cheerio` |
| Framework count / redundancy | **1** | 2 strong | 1 strong |
| Anything abandoned in the list | `voyager` (dead), `sitemap` (2020) | `reppy` (2019), `httpx` (2024) | `bottleneck` (2019), `robots-parser` (2023) |

---

## 4. Rate-limit avoidance — what each library actually implements

Scoring rubric, applied uniformly:

- **Adaptive** — does the *rate itself change* in response to the host getting slower or erroring? (Not "does it retry.")
- **`Retry-After`** — is the header parsed and obeyed?
- **Backoff + jitter** — exponential, and is the sleep randomized?
- **Per-host caps** — concurrency slots and/or token buckets keyed on host, not global.

### 4.1 Scrapy — the only mainstream implementation with true latency-adaptive throttling

**Adaptive: YES, and it is the reference implementation everyone else copies.** `AUTOTHROTTLE_ENABLED` (default `False` — you must switch it on). The algorithm, verbatim from <https://github.com/scrapy/scrapy/blob/master/scrapy/extensions/throttle.py>:

```python
target_delay = latency / self.target_concurrency
new_delay = (slot.delay + target_delay) / 2.0
new_delay = max(target_delay, new_delay)
new_delay = min(max(self.mindelay, new_delay), self.maxdelay)
if response.status != 200 and new_delay <= slot.delay:
    return
slot.delay = new_delay
```

Read that carefully — it is smarter than it looks:
- Delay is derived from **measured latency per download slot** (slot = per-domain by default), so a host that slows down under load automatically gets a longer delay.
- The `max(target_delay, new_delay)` line makes increases fast and decreases slow ("It works better with problematic sites" — their comment).
- The final guard means **non-200 responses can only ever increase the delay, never decrease it.** Error pages are small and fast, so without this guard, getting blocked would *speed the crawler up*. This is exactly the failure mode a naive latency-based throttle has, and Scrapy has already fixed it. **This guard is also the one thing in the whole survey that partially handles the WAF-403 case** — a burst of fast 403s will not accelerate a Scrapy crawl the way it would accelerate a naive one.

Defaults: `AUTOTHROTTLE_START_DELAY = 5.0`, `AUTOTHROTTLE_MAX_DELAY = 60.0`, `AUTOTHROTTLE_TARGET_CONCURRENCY = 1.0`.

**`Retry-After`: NO.** This is a genuine gap and I verified it in source. `scrapy/downloadermiddlewares/retry.py` (`RetryMiddleware.process_response`) is, in full:

```python
if response.status in self.retry_http_codes:
    reason = response_status_message(response.status)
    return self._retry(request, reason) or response
```

`RETRY_HTTP_CODES = [500, 502, 503, 504, 522, 524, 408, 429]` — so 429 *is* retried — but the string `Retry-After` does not appear anywhere in `retry.py`, and `get_retry_request()` re-schedules the request with `priority_adjust = -1` and **no delay of any kind**. The only thing separating the retry from the original request is `DOWNLOAD_DELAY` + AutoThrottle. If a server says `Retry-After: 300`, Scrapy retries in seconds and burns its 2 remaining attempts (`RETRY_TIMES = 2`).
Source: <https://github.com/scrapy/scrapy/blob/master/scrapy/downloadermiddlewares/retry.py>, <https://github.com/scrapy/scrapy/blob/master/scrapy/settings/default_settings.py>

**Backoff + jitter: partial.** No exponential backoff between retries. But `RANDOMIZE_DOWNLOAD_DELAY = True` by default, which randomizes the *inter-request* delay in `0.5×–1.5×` `DOWNLOAD_DELAY` — jitter on the steady-state cadence rather than on retries.

**Per-host caps: YES.** `CONCURRENT_REQUESTS = 16` global, `CONCURRENT_REQUESTS_PER_DOMAIN = 8` per download slot, `DOWNLOAD_DELAY = 0`. Slots are per-domain and AutoThrottle adjusts `slot.delay` per slot.

**Crawl-delay: YES.** Scrapy's `RobotsTxtMiddleware` + Protego surfaces `Crawl-delay`, and Scrapy applies it as the download delay for that domain when `ROBOTSTXT_OBEY` is on.

### 4.2 crawlee-python — the only library that gets `Retry-After` right

This was the biggest surprise of the section. `crawlee-python` has a dedicated `ThrottlingRequestManager` that the JS Crawlee does not have (GitHub code search for `"Retry-After"` returns **9 files in `apify/crawlee-python` and 0 files in `apify/crawlee`**).

**`Retry-After`: YES, correctly, including the HTTP-date form.** `src/crawlee/_utils/http.py::parse_retry_after_header` handles both `delay-seconds` and RFC 7231 HTTP-date, treats naive datetimes as UTC, and explicitly rejects negative delays and past dates with a documented reason ("would push `throttled_until` into the past and silently disable the 429 back-off downstream"). This is the most careful `Retry-After` implementation I found in any of the three ecosystems.
Source: <https://github.com/apify/crawlee-python/blob/master/src/crawlee/_utils/http.py>

**Adaptive: YES for 429s, NO for latency.** `ThrottlingRequestManager.record_domain_delay()`:

```python
state.consecutive_429_count += 1
delay = retry_after if retry_after is not None else self._base_delay * (2 ** (state.consecutive_429_count - 1))
```

Defaults `base_delay = 2s`, `max_delay = 60s`. So: true exponential backoff per domain, `Retry-After` takes priority over the computed backoff, capped. There is **no latency-based throttling** — nothing equivalent to Scrapy's AutoThrottle.
Source: <https://github.com/apify/crawlee-python/blob/master/src/crawlee/request_loaders/_throttling_request_manager.py>

**Jitter: NO.** `base_delay * 2**(n-1)` is deterministic. Two workers hitting the same domain will re-converge.

**Per-host caps: YES, but opt-in and explicitly enumerated.** The manager routes requests into per-domain sub-managers, but only for domains you list: *"Only requests matching these domains will be routed to per-domain sub-managers. Matching is case-insensitive (hostnames are lowercased) and exact: subdomain wildcards such as `*.example.com` are not supported — list each subdomain explicitly if needed."*

**Two loud footguns, both of which the library itself warns about at runtime:**
1. If you get a 429 and are **not** using `ThrottlingRequestManager`, Crawlee logs: *"Received an HTTP 429 (Too Many Requests) response, but the crawler is not using `ThrottlingRequestManager`. Per-domain backoff and `Retry-After` headers will not be honored."* (`_basic_crawler.py` ~line 1597). **The good behaviour is off by default.**
2. `robots.txt` `Crawl-delay` is *also* only enforced through `ThrottlingRequestManager`: *"Crawl-delay directives from robots.txt will not be enforced. To enable crawl-delay support, configure the crawler to use `ThrottlingRequestManager` as the request manager."* (~line 711).

For a project whose stated goal is not getting rate-limited, that is a one-line configuration that changes everything, and it is not the default.

### 4.3 crawlee (JS) — autoscaling that adapts to *your* machine, not to the target host

This is the correction that matters most, because Crawlee's marketing language ("automatically scales concurrency based on system resources") is easy to misread as politeness.

Crawlee JS's `AutoscaledPool` consumes four signals from `Snapshotter` → `SystemStatus`: `memInfo`, `eventLoopInfo`, `cpuInfo`, `clientInfo`. Three of those are your own process. The fourth, `clientInfo`, sounds like the target site but is not:

```ts
const allErrorCounts = options.client.stats?.rateLimitErrors ?? [];
```

…where `client = config.getStorageClient()`. The doc comment is explicit: *"Periodically checks the **storage client** for rate-limit errors (HTTP 429) and reports overload when the error delta exceeds a threshold."* That is the **Apify API**, not the site you are crawling.
Sources: <https://github.com/apify/crawlee/blob/master/packages/core/src/autoscaling/client_load_signal.ts>, <https://github.com/apify/crawlee/blob/master/packages/core/src/autoscaling/snapshotter.ts>, <https://github.com/apify/crawlee/blob/master/packages/core/src/autoscaling/system_status.ts>

**Adaptive w.r.t. the target host: NO.**
**`Retry-After`: NO** — zero occurrences in the repo.
**Backoff + jitter: NO** at the crawler layer; `maxRequestRetries` re-queues without a computed delay. (The underlying `got` client does honour `Retry-After` for 413/429/503 in its own retry layer, but Crawlee's `HttpCrawler` retry loop sits above that.)
**Per-host caps:** `maxRequestsPerMinute` → `AutoscaledPool.maxTasksPerMinute` — a **fixed global cap on the crawler**, not a per-host token bucket. Crawlee's model is one crawler per target, so in practice this is per-host if you structure it that way.

What Crawlee JS does instead is **session rotation**: `SessionPool` with `BLOCKED_STATUS_CODES = [401, 403, 429]` (`packages/core/src/session_pool/consts.ts`, surfaced as `blockedStatusCodes` with that default in `session_pool.ts`), plus `retryOnBlocked` and a `isRequestBlocked` heuristic (`basic-crawler.ts` ~line 892). On a blocked status it retires the session, rotates proxy/cookies/fingerprint, and retries.

**That is a different strategy with a different goal.** It is designed to *evade* rate limiting via identity rotation, not to *avoid* triggering it via politeness. For a `.gov` transparency crawler that intends to identify itself honestly (§2.2), rotating identities is the wrong behaviour, and you would be disabling Crawlee's headline anti-block feature rather than using it.

### 4.4 Rust

**`spider` — adaptive: YES, and it is a direct Scrapy port.** `spider/src/utils/auto_throttle.rs`, doc comment verbatim: *"Inspired by Scrapy's AUTOTHROTTLE — increases delay when servers respond slowly, decreases when they are fast. All operations are lock-free (DashMap + atomics)."* It keeps a per-domain **EMA of response latency** (`alpha = 0.15`) and computes `delay = ema_latency / target_concurrency` clamped to `[min_delay_ms, max_delay_ms]`, defaults `target_concurrency = 2.0`, `min_delay_ms = 0`, `max_delay_ms = 60_000`. Cold start returns `Duration::ZERO`.

Two differences from Scrapy worth flagging: it uses an **EMA** rather than Scrapy's `(old + target)/2` blend, and it does **not** carry Scrapy's `if response.status != 200 and new_delay <= slot.delay: return` guard — so a stream of fast WAF 403s *will* pull spider's computed delay down. Verified against the full source of `delay_for`/`record_latency`.
Source: <https://github.com/spider-rs/spider/blob/main/spider/src/utils/auto_throttle.rs>

`auto_throttle` **is** wired into the crawl loop — `website.rs` holds `auto_throttle: Option<Arc<AutoThrottle>>`, constructs it from `configuration.auto_throttle`, and calls `at.record_latency(domain, page.get_duration_elapsed())` inside the fetch path (`website.rs` ~lines 1505, 4033, 9038). It is in the default feature set (`__basic` includes `auto_throttle`) but is `Option::None` until you set `configuration.auto_throttle`.

**`spider` — `Retry-After`: the code exists but is NOT wired.** `spider/src/utils/rate_limiter.rs` has a per-domain token bucket with an explicit 429 hook:

```rust
/// Called on HTTP 429: reduce the domain's rate to respect the server's
/// `Retry-After` duration. The bucket is drained and the rate is adjusted
/// so roughly one token appears after `retry_after` elapses.
pub fn throttle(&self, domain: &str, retry_after: Duration) { ... }
```

But grepping `website.rs` (~16k lines) for `rate_limit` yields **exactly one hit, and it is a test name** (`fn test_crawl_status_429_is_rate_limited`). The same is true of `adaptive_concurrency.rs` ("AIMD-based adaptive concurrency controller") — a public module the crawl loop does not call. **`DomainRateLimiter` and `AdaptiveConcurrency` are public utilities you must drive yourself, not automatic behaviour.** On a 429 the crawler sets `self.status = CrawlStatus::RateLimited` (`website.rs` ~line 4201) and that is all.
Sources: <https://github.com/spider-rs/spider/blob/main/spider/src/utils/rate_limiter.rs>, <https://github.com/spider-rs/spider/blob/main/spider/src/website.rs>, <https://github.com/spider-rs/spider/blob/main/spider/src/utils/mod.rs>

**`spider` — per-host caps: YES** (token bucket above, if you drive it), plus `configuration.delay` ("Polite crawling delay in milli seconds"), `configuration.concurrency_limit`, `configuration.retry`, and `respect_robots_txt` whose own doc says *"This may slow down crawls if robots.txt file has a delay included"* — i.e. `Crawl-delay` is honoured.
**`spider` — jitter: YES**, `spider/src/utils/backoff.rs`, *"Exponential backoff with jitter for retry logic."*

**Assemble-it-yourself Rust — `reqwest-retry` + `retry-policies`:**
- **429 is retried:** `retryable_strategy.rs` classifies `429 TOO_MANY_REQUESTS` and `408` as transient. Source: <https://github.com/TrueLayer/reqwest-middleware/blob/main/reqwest-retry/src/retryable_strategy.rs>
- **`Retry-After`: NO.** GitHub code search for `"Retry-After"` in `TrueLayer/reqwest-middleware` returns **0 results**. The header is ignored; the policy's own schedule is used.
- **Jitter: YES, and it is the default.** `retry-policies` 0.5.2 `ExponentialBackoff` has a `jitter: Jitter` field with `None | Bounded | Full`, and **`Jitter::Full` is the constructed default**. Source: <https://github.com/TrueLayer/retry-policies/blob/main/src/policies/exponential_backoff.rs>
- **Adaptive: NO.** Fixed policy; the rate does not change based on host behaviour.
- **Per-host caps: not provided** — that is `governor` (0.10.4, MIT, token bucket / GCRA, keyed limiters via `RateLimiter::keyed()`) or `tower::limit`.

**Rust verdict on rate limiting:** the primitives are all present and are individually higher-quality than the Python/TS equivalents (lock-free per-domain EMA, GCRA rate limiting in `governor`, full-jitter backoff by default). What is missing is the **integration** — nothing in Rust ships a crawl loop where 429 → `Retry-After` → per-domain backoff is wired end to end. In Rust you write ~150 lines of glue. In Python (`crawlee-python`) you write one constructor call.

### 4.5 The WAF-403 problem — no library solves it, and that is not a defect

§2.1 established the real failure mode: `www.phila.gov` and `www.sec.gov` return **403 with no `Retry-After`, no 429, and no machine-readable signal**, from CloudFront and AkamaiGHost respectively. Everything in §4.1–4.4 keys on 429 or on latency. Against a 403 CDN block:

- **Scrapy:** 403 is **not** in `RETRY_HTTP_CODES` — the request is dropped, not retried. AutoThrottle's `status != 200` guard prevents the delay from collapsing. Net: it fails safe but silently loses the page.
- **crawlee-python:** `record_domain_delay` is only called on 429. A 403 does nothing.
- **crawlee JS:** the only library that reacts — `BLOCKED_STATUS_CODES = [401, 403, 429]` + `retryOnBlocked` — but its reaction is **identity rotation**, which is the opposite of what an honest `.gov` crawler should do.
- **spider:** the only library that *diagnoses* it. `website.rs::set_crawl_initial_status` inspects the 403 body and distinguishes `WebsiteMetaInfo::RequiresJavascript` (via `is_safe_javascript_challenge`), `WebsiteMetaInfo::Apache403` (`detect_apache_forbidden`), and `WebsiteMetaInfo::OpenResty403` (`detect_open_resty_forbidden`), setting `CrawlStatus::Blocked`. It classifies the block; it does not fix it. Source: <https://github.com/spider-rs/spider/blob/main/spider/src/website.rs>
- **`reqwest-retry`:** `is_retryable` excludes 403 outright.

**This is fundamentally a different problem, and the fix is identification, not backoff.** §2.1 proved it empirically: `curl -A "Centinel Research ben@..."` flipped `www.sec.gov/robots.txt` from 403 to 200, with no change in request rate. The WAF was rejecting the *default `curl` User-Agent*, not the request rate.

So the design conclusion is language-independent:

1. **A descriptive, contactable `User-Agent` is the single highest-value rate-limit mitigation**, and every library in this survey supports setting it (`USER_AGENT` in Scrapy, `additional_http_error_status_codes`/headers in Crawlee, `configuration.user_agent` in spider, `.user_agent()` on a `reqwest::ClientBuilder`).
2. **A per-host policy table — UA string, contact email, req/sec cap, `Retry-After` override, known-403 remediation — is application config that no library provides** (already stated in §2.2, and this section confirms it across all six frameworks).
3. **403 must be a first-class, alerting outcome in the crawl store, not a dropped request.** Only `spider` gives you a typed classification for free; in Python and TS you write the detector.
4. The one thing libraries *can* do for you is not make it worse: prefer a throttle that cannot speed up on error responses. **Scrapy is the only implementation surveyed that has that guard.** If you use `spider`'s `auto_throttle` or write your own, add it.

### 4.6 Scorecard

| | latency-adaptive | 429 → adaptive rate | `Retry-After` | exp. backoff | jitter | per-host cap | `Crawl-delay` | reacts to 403 |
|---|---|---|---|---|---|---|---|---|
| **Scrapy** | **YES** (opt-in) | no | **no** | no | on cadence | YES (8/domain) | YES | drops (no retry) |
| **crawlee-python** | no | **YES** (opt-in) | **YES** | YES | no | YES (opt-in, explicit list) | YES (via same opt-in) | no |
| **crawlee JS** | no (system-only) | no | no | no | no | global `maxRequestsPerMinute` | YES | rotates session |
| **spider (Rust)** | **YES** (opt-in) | code exists, **unwired** | code exists, **unwired** | YES (`backoff.rs`) | YES | YES (token bucket, unwired) | YES | **classifies** |
| **reqwest-retry** | no | retries only | no | YES | **YES (`Jitter::Full` default)** | no (use `governor`) | n/a | no (excluded) |

## 5. robots.txt and sitemap.xml parsers

The §2.4 checklist, restated as a test matrix. A parser passes only if it handles:

1. recursive `<sitemapindex>` nesting (index → index → urlset)
2. query-string `<loc>` values (`sitemap.xml?page=3`)
3. cross-host `Sitemap:` directives (`hillsboroughcounty.org` → `hcfl.gov`)
4. gzip
5. **missing `.xml` extension** (`https://hcfl.gov/sitemap`)
6. XML stylesheet PIs
7. BOM / non-UTF-8

### 5.1 Rust

**robots.txt — two real options, both fine, neither maintained recently.**

| Crate | Version | Released | Repo pushed | License | Notes |
|---|---|---|---|---|---|
| [`texting_robots`](https://crates.io/crates/texting_robots) | 0.2.2 | 2023-03-29 | 2024-02-14 | MIT/Apache-2.0 | 135k DL/90d. **What Firecrawl uses** (§1.2) |
| [`robotstxt`](https://crates.io/crates/robotstxt) | 0.3.0 | **2021-02-13** | **2021-02-13** | Apache-2.0 | Native Rust port of Google's official C++ `robots.txt` parser |

`texting_robots` is the right default. API: `Robot::new(agent: &str, txt: &[u8])` — **takes bytes, not `&str`**, which is the correct signature for the §2.3 reality that some hosts return non-text. It exposes `r.allowed(url) -> bool`, `r.delay: Option<f64>` (Crawl-delay), and `r.sitemaps: Vec<String>`. Wildcards `*` and `$` are supported (README example: `Disallow: /forest*.py`). Its stated design goal is *"a thorough test suite tested against real world data across millions of sites."* It deliberately does **not** fetch or cache — that is yours.
Source: <https://github.com/Smerity/texting_robots/blob/main/README.md>, <https://github.com/Smerity/texting_robots/blob/main/src/lib.rs>

Encoding caveat: `texting_robots` only does UTF-8 validation on `Sitemap:` lines (`Line::Sitemap(url) => String::from_utf8(url.to_vec())`) and otherwise operates on bytes. There is no explicit BOM strip — a UTF-8 BOM before `User-agent:` becomes part of the first token. Minor, but you would want to strip it yourself.

**Sitemaps — this is Rust's weakest spot in the entire survey.**

There is exactly **one** sitemap *parser* crate, and it is `sitemap` 0.4.1, **last released 2020-11-03**, last repo push 2023-05-30. Its own README lists as a restriction: *"no other encodings but UTF-8 are supported yet"* and *"validation is not supported."* It is a streaming reader over `xml-rs` yielding `SiteMapEntity::Url | SiteMap | Err`, so index-vs-urlset discrimination and recursion are things you write.
Source: <https://github.com/svmk/rust-sitemap/blob/master/README.md>, <https://github.com/svmk/rust-sitemap/blob/master/Cargo.toml>

**`sitemap-rs` cannot parse sitemaps.** Despite the name, the newer and better-maintained crate (0.4.0, 2025-08-28) is a **generator and validator only**. Its README says so verbatim: *"This library **cannot** parse sitemaps of any kind (yet! - pull requests welcome!)."* Do not be misled by its release date.
Source: <https://github.com/goddtriffin/sitemap-rs/blob/main/README.md>

`spider` gates its sitemap support behind `sitemap = ["dep:sitemap"]` — i.e. it inherits exactly the 2020 crate above.

**Rust scorecard against the §2.4 checklist:** with `sitemap` 0.4.1 you get (1) partially — it emits `SiteMapEntity::SiteMap` entries but you write the recursion; (2) yes — `<loc>` text passes through; (3) yes — but only because *you* wrote the fetch; (4) **no** — no gzip, that is your `flate2` call; (5) **no** — no content sniffing, that is you; (6) `xml-rs` handles PIs; (7) **no** — UTF-8 only, no BOM handling documented.

The honest read: **in Rust you write the sitemap layer.** ~200–300 lines over `quick-xml` or `roxmltree` + `flate2` + `infer`/magic-byte sniffing. That is not hard, and doing it yourself is arguably *better* given the §2.4 messiness — but it is real work that Python and TS hand you.

*(Note that Firecrawl reached the same conclusion: its Rust native module uses `roxmltree 0.20` directly rather than the `sitemap` crate — see §1.2.)*

### 5.2 Python — the strongest of the three

**robots.txt — `Protego` 0.6.2 (2026-06-25, BSD-3-Clause, pure Python) is the best robots parser in any language surveyed.**

Supported per its README, all demonstrated in a doctest: wildcard `*`, EOL `$`, `Crawl-delay`, **`Request-rate: 10/1m`** (parsed into `RequestRate(requests=10, seconds=60, start_time, end_time)`), `Sitemap`, and `Host` (`preferred_host`). It is Scrapy's default (`ROBOTSTXT_PARSER = "scrapy.robotstxt.ProtegoRobotParser"`) and is separately installable.
Source: <https://github.com/scrapy/protego/blob/master/README.rst>

`Request-rate` support matters: it is the one robots directive that expresses a *rate* rather than a *delay*, and it maps directly onto a token bucket. No Rust or TS parser in this survey parses it.

Avoid `reppy` (0.4.14, **2019-09-16**) — abandoned, and it is a C++ binding. `urllib.robotparser` (stdlib) is present everywhere but does **not** support `*`/`$` wildcards in the Google/RFC 9309 sense and will mis-evaluate the Drupal patterns §2.3 found on `tampa.gov` (`Allow: /core/*.css$`).

**Sitemaps — two good options, and they fail differently.**

**(a) `ultimate-sitemap-parser` (`usp`) 1.8.1, 2026-06-16, GPL-3.0** — <https://github.com/GateNLP/ultimate-sitemap-parser>. Originally Media Cloud's, now maintained by GATE/Sheffield. Field-tested on ~1M URLs. This is the most complete sitemap implementation found in any language.

| §2.4 requirement | usp behaviour | Source |
|---|---|---|
| 1. recursive index | **YES.** `__MAX_RECURSION_LEVEL = 11`, tracks `parent_urls` to break cycles, returns an `AbstractSitemap` tree with `.all_pages()` | `usp/fetch_parse.py` |
| 2. query-string `<loc>` | YES | — |
| 3. cross-host `Sitemap:` | YES — no host filter | — |
| 4. gzip | **YES**, with a graceful fallback: on `GunzipException` it logs *"maybe it's a non-gzipped sitemap"* and proceeds with the raw bytes — exactly right for `.gz` files that aren't gzipped | `usp/helpers.py::ungzipped_response_content` |
| 5. missing `.xml` | **YES** — format detection is content-based: `if response_content[:20].strip().startswith("<")` decides XML vs plain-text sitemap | `usp/fetch_parse.py` line ~182 |
| 6. stylesheet PI | YES — `<?xml-stylesheet?>` still `startswith("<")`; parsed with Expat | — |
| 7. BOM / non-UTF-8 | **BOM yes, other encodings lossy.** `data.decode("utf-8-sig", errors="replace")` — `utf-8-sig` strips the BOM; a `# FIXME other encodings` comment sits directly above the line | `usp/helpers.py` line ~286 |

It also handles plain-text, RSS 2.0, Atom 0.3/1.0, Google News and Image sitemaps, and *"tries to find sitemaps not listed in `robots.txt`"* (well-known-path guessing). Caps: `__MAX_SITEMAP_SIZE = 100 MB`. Custom web client injectable (so you can supply your own UA and rate limiting — important for §2.2).

One weakness vs. Scrapy: gzip detection is `url.path.endswith(".gz") or "gzip" in content-type` — **not** a magic-byte sniff. A gzipped sitemap at an extensionless URL served as `application/xml` would not be decompressed.

**Licence flag: `usp` is GPL-3.0-or-later.** For a project that "ships as a library consumed by a CLI, a server, and a derived MCP," a GPL dependency is a real distribution constraint. This is the single most important non-technical finding in §5.

**(b) Scrapy's built-in `SitemapSpider` + `scrapy.utils.sitemap.Sitemap`** — BSD-3-Clause, and its content detection is *better* than usp's on two axes.

`Sitemap.__init__` uses `lxml.etree.iterparse(..., recover=True, remove_comments=True, resolve_entities=False, remove_blank_text=True, collect_ids=False, remove_pis=True)`. Read that flag list — it is a well-tuned hostile-input parser:
- `recover=True` → malformed XML is recovered rather than fatal
- `remove_pis=True` → **XML stylesheet PIs are stripped outright** (requirement 6, explicitly)
- `resolve_entities=False` → XXE disabled
- `_get_tag_name` does `tag.partition("}")` → namespace-agnostic, so any `xmlns` works

`SitemapSpider._get_sitemap_body` is the content-sniffing layer:
```python
if isinstance(response, XmlResponse):        # content-type says XML
    return response.body
if gzip_magic_number(response):              # response.body[:3] == b"\x1f\x8b\x08"
    ... gunzip(response.body, max_size=...)
if response.url.endswith(".xml") or response.url.endswith(".xml.gz"):
    return response.body
return None                                  # -> "Ignoring invalid sitemap"
```
Source: <https://github.com/scrapy/scrapy/blob/master/scrapy/spiders/sitemap.py>, <https://github.com/scrapy/scrapy/blob/master/scrapy/utils/sitemap.py>, <https://github.com/scrapy/scrapy/blob/master/scrapy/utils/gz.py>

**This passes the Hillsborough case (requirement 5) on the first branch**, not the extension branch: `https://hcfl.gov/sitemap` returns `content-type: application/xml` (§2.4), Scrapy maps that to `XmlResponse`, done. **And it is the only implementation surveyed that sniffs gzip by magic bytes** rather than by extension or declared content-type. It also correctly rejects the `miamidade.gov` case from §2.3 (HTML served where robots/XML expected) — an HTML body with no `.xml` URL returns `None` and logs `Ignoring invalid sitemap`.

Recursion is via `_parse_sitemap` re-queuing itself on `s.type == "sitemapindex"` — unbounded depth, with loop protection delegated to Scrapy's dupefilter. `sitemap_urls_from_robots` reads `Sitemap:` lines from **bytes** and `urljoin`s them, so cross-host absolute URLs pass through unchanged (requirement 3 — pass).

Also worth noting `sitemap_follow` (regex allowlist for which child sitemaps to fetch) and `sitemap_filter()` (an override hook that receives parsed entries — *"you can filter sitemap entries by lastmod greater than a given date"*). **That hook is directly on-mission for Centinel's change tracking**: skip re-fetching children whose `<lastmod>` predates your last run.

**(c) `advertools` 0.18.0 (2026-06-17)** has `sitemap_to_df()` which returns a pandas DataFrame and handles index recursion and gzip. Fine for analysis, wrong shape for a crawler (pandas dependency, eager not streaming).

### 5.3 TypeScript

**robots.txt — `robots-parser` 3.0.1.** Version released **2023-02-21**, but the repo is actively maintained (commits 2026-08-01, 2026-07-17 — dependency bumps). README: *"aims to be compliant with the RFC 9309 specification,"* supports `User-agent`, `Allow`, `Disallow` (with `isExplicitlyDisallowed`), `Sitemap`, `Crawl-delay`, `Host`, and *"paths with wildcards (`*`) and EOL matching (`$`)"*. Feature-complete for §2.3's Drupal patterns. This is also what **Firecrawl uses on its TS side** (§1.9).
Source: <https://github.com/samclarke/robots-parser/blob/master/README.md>

**Sitemaps — Crawlee's `parseSitemap` is the best-engineered sitemap fetcher in TS, with one default that will silently break a `.gov` case.**

`packages/utils/src/internals/sitemap.ts`:

| §2.4 requirement | Crawlee behaviour | Detail |
|---|---|---|
| 1. recursive index | **YES.** `maxDepth = Infinity` default, `visitedSitemapUrls: Set` breaks cycles, `sitemapRetries = 3` per sitemap | — |
| 2. query-string `<loc>` | YES — `text.trim()`, no normalization | — |
| 3. cross-host `Sitemap:` | **NO by default.** See below | — |
| 4. gzip | **YES, magic-byte sniffed** via `fileTypeStream` from the `file-type` package, then `createGunzip()`; also strips a `.gz` suffix from the URL before re-sniffing type | — |
| 5. missing `.xml` | **YES** — `new MIMEType(contentType).isXML()` (whatwg-mimetype, so `application/xml`, `text/xml`, and any `+xml`) OR `.xml` path suffix; falls back to `text/plain`/`.txt` for text sitemaps; otherwise throws `Unsupported sitemap content type` | — |
| 6. stylesheet PI | YES — `sax` handles PIs as a distinct event, not an error | — |
| 7. BOM | YES — sax.js skips a leading `\uFEFF` (`lib/sax.js` line ~1228). **Non-UTF-8: no** — `new StringDecoder('utf8')` is hardcoded | — |

**The trap.** Both `RobotsTxtFile.getSitemaps()` and `parseSitemap()` apply an `enqueueStrategy` that **defaults to `'same-hostname'`**:

```ts
getSitemaps(options: RobotsTxtFileSitemapsOptions = {}): string[] {
    const { enqueueStrategy = 'same-hostname' } = options;
    for (const sitemapUrl of this.robots.getSitemaps()) {
        const { allowed, reason } = filterUrl(sitemapUrl, this.url, enqueueStrategy);
        if (!allowed) { log.warning(`Skipping sitemap ${sitemapUrl} listed in robots.txt at ${this.url}: ${reason}.`); continue; }
        sitemaps.push(sitemapUrl);
    }
    return sitemaps;
}
```

**This drops the exact case §2.4 documented.** `www.hillsboroughcounty.org/robots.txt` declares `Sitemap: https://hcfl.gov/sitemap`; `hcfl.gov` is not the same hostname as `www.hillsboroughcounty.org`, so out of the box Crawlee logs a warning and **discards the county's entire sitemap**. The same filter is applied to nested `<sitemap><loc>` entries inside an index (a common CDN pattern) and to `<url><loc>` entries. The fix is one option — `{ enqueueStrategy: 'all' }` — but the failure is silent-ish (a `log.warning`, not an error) and the default is wrong for federated `.gov` estates.
Source: <https://github.com/apify/crawlee/blob/master/packages/utils/src/internals/robots.ts>, <https://github.com/apify/crawlee/blob/master/packages/utils/src/internals/sitemap.ts>

One more: the XML parser is `new sax.SAXParser(true)` — **strict mode**, with `this.parser.onerror = this.destroy.bind(this)`. Malformed XML kills the stream (retried up to `sitemapRetries` times, then abandoned). Compare Scrapy's `recover=True`, which salvages what it can. For messy municipal sitemaps, Scrapy's posture is the safer one.

Standalone alternatives: `sitemapper` 4.1.6 (2026-05-10, MIT) — simple, handles index recursion and gzip, `timeout`/`concurrency`/`fields` options; and `sitemap` 9.0.1 (2026-02-28, MIT, `ekalinin/sitemap.js`) — primarily a *generator* but does ship `XMLToSitemapItemStream` for parsing. Neither is as thorough as Crawlee's.

### 5.4 Verdict for §5

| §2.4 requirement | Rust | Python (Scrapy) | Python (usp) | TypeScript (Crawlee) |
|---|---|---|---|---|
| 1. recursive index | you write it | YES (unbounded) | YES (depth 11) | YES (`maxDepth`) |
| 2. query-string `<loc>` | YES | YES | YES | YES |
| 3. cross-host `Sitemap:` | YES (you write it) | YES | YES | **NO by default** |
| 4. gzip | you write it | **YES (magic bytes)** | YES (ext/content-type) | **YES (magic bytes)** |
| 5. missing `.xml` | you write it | **YES (content-type)** | **YES (content sniff)** | YES (MIME/`+xml`) |
| 6. stylesheet PI | via `xml-rs`/`roxmltree` | **YES (`remove_pis`)** | YES | YES (sax) |
| 7. BOM / non-UTF-8 | **NO** (UTF-8 only) | YES / lxml handles | BOM yes / lossy | BOM yes / UTF-8 only |
| malformed XML | strict | **recovers** | Expat (strict-ish) | **strict, destroys stream** |
| Licence | MIT | BSD-3 | **GPL-3.0** | Apache-2.0 |

**Nobody passes all seven.** Scrapy comes closest and is the only one that both sniffs gzip by magic bytes *and* recovers from malformed XML. Crawlee is close behind but ships a default that breaks a real case from §2.4. Rust hands you a 2020 crate and a to-do list.

## 6. HTML → markdown conversion

Two distinct jobs that people conflate:

- **Boilerplate removal / main-content extraction** — decide *which subtree* is the article and drop nav, header, footer, sidebar, cookie banner, "skip to main content".
- **Serialization** — turn that subtree into markdown, preserving links, tables, lists, code.

Firecrawl does both, in that order, deterministically (§1.3: `onlyMainContent` is *"a deterministic HTML-level filter applied before markdown is generated; no LLM is involved"*). Some libraries below do both; most do exactly one.

**Licence warning up front, because it recurs:** two of the most commonly recommended converters are **GPL** — `html2md` (Rust, `GPL-3.0+`) and `html2text` (Python, `GPL-3.0-or-later`). Both were verified from the registry metadata (crates.io version `license` field; PyPI `license_expression`). For a project shipping as a consumable library, both should be off the table. There are permissive equivalents in each language, so this costs nothing.

### 6.1 Rust

| Crate | Version | Released | License | Job | Notes |
|---|---|---|---|---|---|
| [`htmd`](https://crates.io/crates/htmd) | 0.5.5 | 2026-07-27 | Apache-2.0 | serialize | 1.46M DL/90d, 450 stars |
| [`fast_html2md`](https://crates.io/crates/fast_html2md) | 0.0.62 | 2026-04-30 | MIT | serialize | spider-rs; `lol_html` rewriter |
| [`html2md`](https://crates.io/crates/html2md) | 0.2.15 | 2025-01-12 | **GPL-3.0+** | serialize | **avoid — copyleft** |
| [`dom_smoothie`](https://crates.io/crates/dom_smoothie) | 0.18.0 | 2026-06-07 | MIT | boilerplate | Port of `readability.js`; 213k DL/90d |
| [`llm_readability`](https://crates.io/crates/llm_readability) | 0.0.17 | 2026-04-30 | — | boilerplate | spider-rs |
| [`spider_transformations`](https://crates.io/crates/spider_transformations) | 2.39.13 | 2026-04-30 | — | **both** | See below |

**`htmd` is the best pure serializer in Rust and it is not close.** Its own README claims, and the crate is built around, turndown.js parity: *"Rich options, same as turndown.js"*, *"Reliable, it passes all test cases of turndown.js"*, *"**HTML table to Markdown table conversion**"*, *"Minimum dependencies, it uses only html5ever"*, *"~16ms to convert a 1.37MB Wikipedia page on Apple M4"*, plus a *"faithful mode, which can preserve HTML output for tags not supported by Markdown."*
Source: <https://github.com/letmutex/htmd/blob/main/README.md>

For boilerplate it gives you `skip_tags(vec!["script", "style"])` and custom per-tag handlers — **tag-level filtering, not content extraction.** It will not find the `<main>` of a CivicPlus page for you. **`htmd` needs a second step.**

**`dom_smoothie` is that second step.** *"A Rust crate for extracting readable content from web pages… closely follows the implementation of readability.js, bringing its functionality to Rust."* It returns an `Article` with `title`, `byline`, `length`, `excerpt`, `site_name`, `dir`, `published_time`, **`modified_time`**, `image`, `url` — and the `document_url` parameter *"may be used to transform relative URLs into absolute URLs."* That metadata set matches what Firecrawl returned in §1.5, including the `modifiedTime` field that is directly useful for change tracking.
Source: <https://github.com/niklak/dom_smoothie/blob/main/README.md>

So the canonical Rust pipeline is **`dom_smoothie` → `htmd`**: two crates, both MIT/Apache, both actively maintained (pushed 2026-06-07 and 2026-07-27), total dependency surface `html5ever` + `tendril` + friends. No native deps, no subprocess.

**`spider_transformations` is the one Rust crate that does both in one call**, and its dependency list is a good map of the ecosystem: `fast_html2md` (serialize) + `llm_readability` (boilerplate) + `lol_html` (rewriting) + **`auto_encoder`** (charset detection — relevant to §2.4 requirement 7) + optional `calamine` + `zip` + `quick-xml` (Office documents) + optional `whisper-rs` + `symphonia` (audio transcription). If you are already using `spider`, this is free. If you are not, it drags in `spider ^2` as a hard dependency.
Source: crates.io dependency listing for `spider_transformations` 2.39.13.

### 6.2 Python

| Package | Version | Released | License | Job |
|---|---|---|---|---|
| [`trafilatura`](https://pypi.org/project/trafilatura/) | 2.2.0 | 2026-07-31 | Apache-2.0 | **both** |
| [`markdownify`](https://pypi.org/project/markdownify/) | 1.2.3 | 2026-06-30 | MIT | serialize |
| [`html2text`](https://pypi.org/project/html2text/) | 2025.4.15 | 2025-04-15 | **GPL-3.0-or-later** | serialize — **avoid** |

**`trafilatura` is the single strongest boilerplate-removal implementation in this entire survey**, and it emits markdown directly — one step, no second library.

Its own framing: *"Going from HTML bulk to essential parts can alleviate many problems related to text quality, by focusing on the actual content, avoiding the noise caused by recurring elements like headers and footers."* It layers *"common patterns and generic algorithms like jusText and readability"* and its README asserts *"Trafilatura consistently outperforms other open-source libraries in text extraction benchmarks"* (their own evaluation, so weight accordingly, but the adoption list — HuggingFace, IBM, Microsoft Research, Allen Institute, Stanford — is independent evidence).
Source: <https://github.com/adbar/trafilatura/blob/master/README.md>

**Its defaults will silently destroy what Centinel needs, and this is the most actionable finding in §6.** From `trafilatura/settings.py`, the `Extractor` constructor:

```python
output_format: str = "txt",
comments: bool = True,
formatting: bool | None = None,
links: bool = False,      # <-- links are DISCARDED by default
images: bool = False,     # <-- images are DISCARDED by default
tables: bool = True,      # <-- tables are kept
```
…and `self.formatting = (self.format == "markdown") if formatting is None else formatting`.
Source: <https://github.com/adbar/trafilatura/blob/master/trafilatura/settings.py>

So: **tables are on, links and images are off.** For a crawl store whose whole purpose is a durable, linked, diffable record of a government page, `include_links=True` is mandatory and is not the default. (It also means you cannot use trafilatura's markdown output to harvest outbound links for the frontier unless you flip it.)

`trafilatura` also emits XML/XML-TEI, JSON, CSV, and extracts metadata (title, author, date, sitename, categories, tags) — which is more structure than markdown alone would carry, and worth considering for the sidecar metadata in Centinel's store.

**`markdownify` 1.2.3 (MIT) is the permissive serializer.** BeautifulSoup-based, so it inherits `bs4`'s tolerance for broken HTML. Relevant options confirmed from its README: `strip` / `convert` (tag allow/deny lists), `heading_style` (ATX/SETEXT), `newline_style`, `code_language_callback`, `table_infer_header` (*"Controls handling of tables with no header row (as indicated by `<thead>`)"* — directly useful, government tables frequently omit `<thead>`), `keep_inline_images_in`, and `strip_document` (LSTRIP/RSTRIP/STRIP). Tables are supported.
Source: <https://github.com/matthewwithanm/python-markdownify/blob/develop/README.rst>

Like `htmd`, `markdownify` does **tag filtering, not content extraction** — pair it with `trafilatura` (extract HTML, then serialize) or `readability-lxml`.

**`html2text` is GPL-3.0-or-later.** It is the oldest and most-recommended option and it is the wrong one for a shipped library. Use `markdownify`.

### 6.3 TypeScript

| Package | Version | Released | License | Job |
|---|---|---|---|---|
| [`turndown`](https://www.npmjs.com/package/turndown) | 7.2.4 | 2026-04-03 | MIT | serialize |
| [`@mozilla/readability`](https://www.npmjs.com/package/@mozilla/readability) | 0.6.0 | **2025-03-03** | Apache-2.0 | boilerplate |
| [`defuddle`](https://www.npmjs.com/package/defuddle) | 0.19.2 | 2026-07-22 | MIT | **both** |
| [`node-html-markdown`](https://www.npmjs.com/package/node-html-markdown) | 2.0.0 | 2025-11-14 | MIT | serialize |

**`turndown` is the reference implementation that everything else is measured against** — 11,369 stars, and both `htmd` (Rust) and Firecrawl's JS fallback path (§1.5) target its behaviour. GFM tables require the `turndown-plugin-gfm` plugin (Firecrawl uses `joplin-turndown-plugin-gfm`); **base turndown does not emit markdown tables.** That is a real gap versus `htmd` and `markdownify`, which handle tables natively.

**`@mozilla/readability` is the canonical boilerplate remover**, and it is the origin of `dom_smoothie` (Rust) and of readability ports everywhere. But note: **0.6.0 released 2025-03-03** — nearly 17 months without a release, though the repo is active (pushed 2026-07-09). It requires a DOM, so in Node it needs `jsdom` or `linkedom`.

**`defuddle` 0.19.2 is the interesting newcomer** — Obsidian Web Clipper's extractor, 8,717 stars, MIT, released 2026-07-22. It does **both** jobs: *"takes a URL or HTML, finds the main content, and returns cleaned HTML or Markdown."* Positioned explicitly as a Readability replacement with four stated differences: *"More forgiving, removes fewer uncertain elements. Provides a consistent output for footnotes, math, code blocks, etc. Uses a page's mobile styles to guess at unnecessary elements. Extracts more metadata from the page, including schema.org data."*

Two caveats, both from its own README: it opens with **"Beware! Defuddle is very much a work in progress!"**, and `defuddle/node` needs an externally-supplied DOM (`jsdom`, `linkedom`, `happy-dom`).
Source: <https://github.com/kepano/defuddle/blob/main/README.md>

"More forgiving, removes fewer uncertain elements" is arguably the *right* bias for an archival crawler — Readability is tuned for news articles and is aggressive; a council agenda page with a sidebar of meeting dates is exactly the sort of page Readability over-trims.

The "extracts schema.org data" claim also matters for §8: government CMS pages frequently emit JSON-LD (`Event`, `GovernmentOrganization`, `Dataset`), and that structured data is often better than the rendered HTML.

### 6.4 Verdict for §6

| | one-step (boilerplate + markdown) | best serializer | best boilerplate remover | tables native | permissive licence throughout |
|---|---|---|---|---|---|
| **Rust** | `spider_transformations` (drags in `spider`) | **`htmd`** (turndown parity) | `dom_smoothie` | **YES** (`htmd`) | yes, if you avoid `html2md` |
| **Python** | **`trafilatura`** | `markdownify` | **`trafilatura`** (best in survey) | YES (both) | yes, if you avoid `html2text` |
| **TypeScript** | `defuddle` (self-described WIP) | `turndown` | `@mozilla/readability` (stale releases) | **NO** — turndown needs a GFM plugin | yes |

Practical read:
- **Python wins on extraction quality** (`trafilatura` is genuinely state of the art, with published benchmarks and institutional adoption) but ships defaults that discard links.
- **Rust wins on serialization quality** (`htmd` claims and tests turndown parity *including tables*, with `html5ever` as its only dependency) and now has a credible Readability port in `dom_smoothie`. This is a category where the Rust ecosystem is **not** weaker — it is arguably ahead of TS.
- **TypeScript is the only one where the default serializer cannot do tables without a plugin**, and its boilerplate options are either stale (`@mozilla/readability`) or self-declared work-in-progress (`defuddle`).

For `.gov` content specifically — budget tables, fee schedules, meeting rosters — **table fidelity is not a nice-to-have**, which pushes against bare turndown and toward `htmd` / `markdownify` / `trafilatura(tables=True)`.

## 7. JS-rendered pages

### 7.1 What the options are, per language

| | Playwright | Puppeteer | Native CDP client | WebDriver |
|---|---|---|---|---|
| **TypeScript** | `playwright` 1.62.1 (2026-07-30, Apache-2.0) — **first-party** | `puppeteer` 25.4.0 (2026-07-27, Apache-2.0) — **first-party** | — | — |
| **Python** | `playwright` 1.62.0 (2026-07-31, Apache-2.0) — **first-party, Microsoft-published** | `pyppeteer` — unofficial, stale | — | Selenium |
| **Rust** | `playwright` 0.0.20 — **dead, and a subprocess wrapper** (see below) | — | **`chromiumoxide` 0.9.1**, `headless_chrome` 1.0.22, `spider_chrome` 2.37.130 | `fantoccini` 0.22.1, `thirtyfour` 0.37.4 |

**Rust has no first-party Playwright, and the community one is both dead and architecturally wrong for this.** `playwright` 0.0.20 on crates.io was last released **2022-08-20**; the repo was last pushed **2024-05-04**. More importantly its own README states what it is: *"Playwright is a rust library to automate Chromium, Firefox and WebKit **built on top of Node.js library**."* It drives the Node Playwright driver as a child process. Choosing it means shipping a **Node.js runtime plus the Playwright npm package plus the browser binaries** alongside a Rust binary — the worst of every world.
Source: <https://github.com/octaltree/playwright-rust>, <https://crates.io/crates/playwright>

**This is the clearest case in the whole survey where Rust forces a different architecture rather than a worse library.** The Rust answer is not "wrap Playwright," it is "speak CDP directly":

- **`chromiumoxide` 0.9.1** (2026-02-25, MIT/Apache-2.0, 1.43M DL/90d, 1,360 stars) — *"provides a high-level and async API to control Chrome or Chromium over the DevTools Protocol. It comes with support for all types of the Chrome DevTools Protocol and can launch a headless or full Chrome or Chromium instance **or connect to an already running instance**."* Pure Rust, tokio/async-std, no Node. Repo pushed 2026-04-03, 58 open issues.
  Source: <https://github.com/mattsse/chromiumoxide>
- **`spider_chrome` 2.37.130** — spider-rs's hard fork of chromiumoxide, kept in lockstep with `spider`. This is what `spider`'s `chrome` feature uses.
- **`headless_chrome` 1.0.22** (2026-06-11, MIT, 892k DL/90d, 2,939 stars) — the sync alternative. 143 open issues.
- **`fantoccini` / `thirtyfour`** — WebDriver clients; require a separate `geckodriver`/`chromedriver` process. Adds a moving part rather than removing one.

`chromiumoxide` **does not download a browser for you.** You point it at an installed Chrome/Chromium. That is a downside for turnkey install and an upside for containerized deployment — it composes cleanly with `mcr.microsoft.com/playwright` or a distro `chromium` package, and it means the crate itself adds ~0 MB.

### 7.2 Dependency weight — the real numbers

From Playwright's own docs (`docs/src/browsers.md`), verbatim `du -hs` output:

```
281M  chromium-XXXXXX
187M  firefox-XXXX
180M  webkit-XXXX
```

And from `packages/playwright-core/browsers.json`, **`installByDefault: true`** is set on `chromium` (152.0.7977.8), `chromium-headless-shell`, `firefox` (153.0), `webkit` (26.5), and `ffmpeg`. A bare `npx playwright install` therefore pulls **~650 MB+** before you have crawled anything.
Sources: <https://github.com/microsoft/playwright/blob/main/docs/src/browsers.md>, <https://github.com/microsoft/playwright/blob/main/packages/playwright-core/browsers.json>

Playwright provides the knobs to cut this, and Centinel should use them:
- `npx playwright install chromium` — one browser only.
- **`playwright install --with-deps --only-shell`** — *"If you are only running tests in headless shell (i.e. the `channel` option is not specified)… you can avoid downloading the full Chromium browser by passing `--only-shell` during installation."* `chromium-headless-shell` is materially smaller than full Chromium.
- `--no-shell` — the inverse, if you want the new Chrome headless mode via `channel: 'chromium'`.
- `PLAYWRIGHT_BROWSERS_PATH` — relocate/share the cache across containers and users.

Behavioural caveat straight from the docs, worth recording because it will bite: *"Google Chrome and Microsoft Edge have switched to a new headless mode implementation that is closer to a regular headed mode. This differs from chromium headless shell that is used in Playwright by default when running headless, so expect different behavior in some cases."* (<https://github.com/microsoft/playwright/issues/33566>). Some WAFs (§2.1) fingerprint headless-shell specifically.

### 7.3 Can it be an *optional* runtime feature?

Yes in all three, but the mechanisms differ sharply in how well they actually keep the dependency out.

**Rust — cleanest.** Cargo features gate the code at *compile* time. `spider` already models this: `chrome`, `chrome_headless`, `chrome_cpu`, `chrome_intercept`, `real_browser` etc. are opt-in features, and `spider_chrome` is not compiled in unless requested. A Centinel build without `--features chrome` contains no CDP code at all, and the binary does not grow. Because `chromiumoxide` never downloads a browser, "optional" means genuinely zero footprint by default and "bring your own Chrome" when enabled. **This is a real Rust advantage, not a consolation prize.**

**Python — good.** Optional extras (`pip install centinel[browser]`) plus a deferred `import playwright` inside the render path. The `playwright` wheel itself is small; the weight is the separate `playwright install` step, which is already a distinct user action. Crawlee-Python models this exactly: `crawlee[playwright]` is an extra, and `PlaywrightCrawler` lives in its own module.

**TypeScript — the messiest.** npm has no compile-time feature gating; the options are `optionalDependencies`, `peerDependencies` with `peerDependenciesMeta.optional`, or a dynamic `await import('playwright')`. All three work, but all three are runtime contracts you have to document and error-check, and `npm install` behaviour around optional peers is historically inconsistent across package managers. Additionally `puppeteer` (as opposed to `puppeteer-core`) **downloads a browser in a postinstall script by default** — the `puppeteer` package depends on `@puppeteer/browsers` 3.0.6 for exactly that. `puppeteer-core` is the no-download variant, and is what a library should depend on.

### 7.4 What Centinel actually needs from JS rendering

Worth stating plainly, because it changes the weight of this section: **`.gov` content is overwhelmingly server-rendered.** The §2.4 evidence supports this — Tampa is Drupal with a 12,000-URL sitemap of real HTML URLs; CDC, IRS, EPA all publish static-ish pages. The places JS rendering becomes mandatory are the **vendor CMS applications** in §8 — Legistar/Granicus agenda portals, ArcGIS Hub, and PrimeGov meeting viewers — and §8 shows that most of those expose an API that is a *better* answer than rendering their SPA.

`spider` also gives you a cheap signal for when rendering is actually required: `WebsiteMetaInfo::RequiresJavascript` via `is_safe_javascript_challenge` (§4.5). The right design is **HTTP-first, escalate to a browser only on a detected JS requirement or a configured per-host override** — which is exactly the shape that makes browser support optional in the first place.

## 8. Government CMS platforms — where an API beats crawling

Everything in this section was **probed live on 2026-08-02** with `curl` and a descriptive User-Agent, against production hosts. Status codes, byte counts and record shapes are measured, not quoted.

**Headline: this section changes the crawl design.** Three of the seven platforms expose keyless, structured, machine-readable interfaces that are strictly better than parsing their HTML — and one of them (Legistar) supports `$filter` on a last-modified timestamp, which is *exactly* the change-detection primitive Centinel is otherwise going to build by hashing markdown.

### 8.1 Legistar / Granicus — **the best case, and it is very good**

**Confirmed: the Legistar Web API is real, public, keyless for most clients, and OData-queryable.**

Base: `https://webapi.legistar.com/v1/{Client}/...` — the client name is the same slug as `{client}.legistar.com`.

Live probe results:

| Client | `GET /v1/{client}/bodies?$top=1` |
|---|---|
| `seattle` | **200**, JSON array of `Body` records |
| `sfgov` | **200** |
| `oakland` | **200** |
| `sanjose` | **200** |
| `mesa` | **200** |
| `chicago`, `philadelphia`, `tampa` | 500 — `"LegistarConnectionString setting is not set up in InSite for client: X"` (not a Legistar client under that slug) |
| `nyc`, `phila` | **403** — CDN/WAF, no body (the §2.1 pattern again) |

**No API key was used.** The docs say *"some clients require use of an API Token"* and it *"can be provided as a URL parameter"* (`?token=verylongbase64token`), but the five clients above answered anonymously.

**Endpoint surface** — parsed from the live ASP.NET Web API help page at <https://webapi.legistar.com/Help>: **124 documented operations, 55 of them `GET`**, across these root resources:

`Actions`, `Bodies`, `BodyTypes`, `CodeSections`, `EventDates`, `EventItems`, `Events`, `Indexes`, `MatterIndexes`, `MatterRequesters`, `MatterStatuses`, `MatterTypes`, `Matters`, `OfficeRecords`, `Persons`, `VoteTypes`

Notable sub-resources, verbatim from the help page:
- `GET v1/{Client}/Matters/{MatterId}/Attachments` — *"Gets all available for Internet viewing Matter Attachments for one Matter record"*
- `GET v1/{Client}/Matters/{MatterId}/Attachments/{MatterAttachmentId}/File` — **"Gets the file content for one Matter Attachment."** ← *this is a direct, addressable PDF/document fetch, no HTML scraping*
- `GET v1/{Client}/Matters/{MatterId}/Histories?AgendaNote=&MinutesNote=` — legislative history per matter
- `GET v1/{Client}/Events/{EventId}/EventItems?AgendaNote=&MinutesNote=&Attachments=` — full agenda, with attachments, in one call
- `GET v1/{Client}/EventDates/{BodyId}?FutureDatesOnly=` — meeting calendar per body
- `GET v1/{Client}/eventitems/{id}/votes` — roll-call votes

**OData support, verified live.** This query returned real filtered data from Seattle:

```
GET https://webapi.legistar.com/v1/seattle/matters
      ?$select=MatterId,MatterFile,MatterTitle,MatterLastModifiedUtc
      &$filter=MatterLastModifiedUtc gt datetime'2026-07-01'
      &$top=3
→ 200, e.g. {"MatterLastModifiedUtc":"2026-07-15T19:05:02.17","MatterFile":"CB 118502","MatterTitle":"AN ORDINANCE establishing a new Office of Planning and Community Development…","MatterId":2914}
```

`$select`, `$filter`, `$top`, `$skip` all work. The documented examples confirm the pattern:
- `…/matters?$top=10&$skip=0` then `…/matters?$top=10&$skip=10` (paging)
- `…/events?$filter=EventDate+ge+datetime'2014-09-01'+and+EventDate+lt+datetime'2014-10-01'`
- `…/matters/1234/histories?$filter=MatterHistoryPassedFlag ne null and MatterHistoryActionBodyName eq 'Common Council'`

Source: <https://webapi.legistar.com/Home/Examples>, <https://webapi.legistar.com/Help>, <https://support.granicus.com/s/article/Legistar-Web-API>

**Hard limit, measured:** a request with no `$top` returned exactly **1000 records** for `seattle/matters` — matching the documented *"queries replies are limited to 1000 responses."* Paging via `$skip` is mandatory for full extraction. (Oddity worth knowing: `$top=2000` returned **4** records, not 1000 — do not exceed 1000 in `$top`.)

**Every entity carries `...LastModifiedUtc` and `...RowVersion` fields** (visible in the raw `Body` records: `BodyLastModifiedUtc`, `BodyRowVersion`; likewise `MatterLastModifiedUtc`). **This is a server-side change feed.** For any Legistar jurisdiction, Centinel's "what changed since last run" question is one OData filter, not a re-crawl and re-hash of thousands of pages.

**Design consequence:** for `.gov` targets running Legistar, the crawler should be a *thin OData client*, and the HTML site (`{client}.legistar.com`) should be crawled only to capture the rendered presentation, if at all.

**Granicus non-Legistar (media/agenda publishing) also has a feed.** Verified live:

```
GET https://miamifl.granicus.com/ViewPublisherRSS.php?view_id=1&mode=agendas
→ 200, content-type: text/xml, 79,416 bytes, RSS 2.0, 100 <item> elements
```

Each item carries a stable `<guid isPermaLink="false">` (a UUID), `<title>`, `<pubDate>`, a Granicus-namespaced `<gran:pubDateParts yr= mo= day= hr= min= sec= tz=/>`, and a `<link>` to `AgendaViewer.php?view_id=N&clip_id=M`. Namespace: `xmlns:gran="https://www.granicus.com/schema/rss-supplements"`. `mode=` also accepts other values (the HTML archive at `ViewPublisher.php?view_id=1` for the same client is a **5.98 MB single page** — the RSS feed is dramatically cheaper).

Note this feed opens with `<?xml-stylesheet href="browserfriendlyRSS_Layout.xslt" type="text/xsl" media="screen"?>` — **a live instance of §2.4 requirement 6**, on a platform Centinel will definitely encounter.

Granicus client slugs are unpredictable (`miamifl` works; `tampafl`, `sanjose`, `austintx`, `oaklandca`, `bostonma` all 404 on `view_id=1`), so slug + `view_id` must be per-host configuration, discovered once and stored.

### 8.2 ArcGIS Hub — **a full catalog API and a DCAT feed**

**Confirmed live, keyless.**

**(a) Hub Search API** (OGC API Features shaped):
```
GET https://hub.arcgis.com/api/search/v1/collections/dataset/items?limit=1
→ 200, content-type: application/geo+json, a GeoJSON FeatureCollection
```
Each feature's `properties` carries `title`, `type`, `typeKeywords`, `description`, `snippet`, `licenseInfo`, `access`, `extent`, `modified`, `categories`, and more.

**(b) DCAT-US 1.1 catalog feed**, per-site:
```
GET https://gis-fdot.opendata.arcgis.com/api/feed/dcat-us/1.1.json
→ 200, application/json, 1,549,398 bytes
GET https://hub.arcgis.com/api/feed/dcat-us/1.1.json
→ 200, application/json, 24,980,498 bytes   (the whole hub)
```

DCAT-US is the **federally standardized open-data catalog schema** (the same one `data.gov` harvests). Any ArcGIS Hub site therefore publishes a machine-readable inventory of every dataset, with distributions, formats, and modification dates, at a predictable path: `{site}/api/feed/dcat-us/1.1.json`.

**Design consequence:** ArcGIS Hub sites should **never** be HTML-crawled for their dataset inventory. Fetch the DCAT feed, diff it, and pull the changed distributions. Note the sites also live on custom domains — the `.../api/feed/dcat-us/1.1.json` path works on both `*.opendata.arcgis.com` and vanity domains, but a bare municipal domain redirect (`data.tampagov.net` → 301) needs following.

### 8.3 PrimeGov — **an undocumented but wide-open public JSON API**

**Confirmed live, keyless, on two independent tenants.**

```
GET https://lacity.primegov.com/api/v2/PublicPortal/ListArchivedMeetings?year=2026
→ 200, application/json, 556 records
GET https://longbeach.primegov.com/api/v2/PublicPortal/ListArchivedMeetings?year=2026
→ 200, application/json
GET https://lacity.primegov.com/api/v2/PublicPortal/ListUpcomingMeetings
→ 200, application/json, 14,992 bytes
GET https://lacity.primegov.com/api/meeting/search?from=1/1/2026&to=8/1/2026
→ 200, application/json, 360,033 bytes
```

Record shape (verbatim field names from the live response):
```json
{ "id": 17670, "meetingTypeId": 42, "committeeId": 15,
  "dateTime": "2026-01-02T09:00:00", "date": "Jan 02, 2026", "time": "09:00 AM",
  "documentList": [
    { "id": 78499, "language": "en-US", "compileOutputType": 3, "publishStatus": 1,
      "publishDate": "2025-12-16T21:32:43.6", "templateId": 149492, "meetingId": 17670,
      "sortOrder": 1, "templateName": "HTML Notice of Cancellation" }
  ],
  "allowPublicSpeaker": false, "allowPublicComment": false, "isZoomMeeting": false,
  "videoUrl": null, "swagitId": null, "meetingState": 3, "publishDate": null,
  "title": "Rules, Elections and Intergovernmental Relations Committee", "location": "" }
```

`documentList[].publishDate` is a per-document timestamp — again a change signal without re-crawling.

**Caveat: this is an undocumented internal API for their SPA.** There is no published contract, no versioning promise beyond the `v2` in the path, and no stated terms. It is public and unauthenticated, but treat it as a best-effort optimization with an HTML fallback, and be conservative with request rates.

### 8.4 Municode — **a public JSON API behind the code library**

**Confirmed live, keyless:**
```
GET https://api.municode.com/Clients/stateAbbr?stateAbbr=FL
→ 200, application/json, 174,194 bytes, 416 client records
```
Each record: `ClientID`, `ClientName`, `State{StateID,StateName,StateAbbreviation}`, `Address`, `City`, `ZipCode`, `Website`, `PopRangeId`, `ClassificationId`, `ShowAdvanceSheet`, `LibraryHomePageTemplateName`, `Meetings`.

`api.municode.com` is the backend for the `library.municode.com` SPA; `GET /CodesContent` exists (returns `400 application/problem+json` without correct parameters — i.e. a real, validating endpoint, not a 404). **`library.municode.com/fl/tampa` returns only 6,095 bytes of HTML** — an SPA shell. Crawling Municode as HTML gets you nothing; you must either use the API or render JS.

This is another undocumented internal API. Same caution as PrimeGov applies, but the client-directory endpoint alone is valuable: it is a **complete machine-readable roster of every Municode jurisdiction per state**, which is a discovery source for Centinel's target list.

### 8.5 Laserfiche WebLink / Public Portal — **no anonymous API**

Laserfiche does publish a REST API, but it is the wrong one for this purpose. Per <https://developer.laserfiche.com/docs/api/guide_overview-of-the-laserfiche-api/>, the Laserfiche API is *"a set of RESTful web APIs that allow you to build integrations between third-party applications and **Laserfiche Cloud** services,"* with two sets — the **repository API** and the **table API** (OData v4). It requires **OAuth 2.0** credentials against a Laserfiche Cloud tenant.

**WebLink / Public Portal are the self-hosted public-facing HTML viewers**, and they do not expose that API anonymously. Their surface is ASP.NET pages: `WebLink/Browse.aspx?dbid=0`, `WebLink/DocView.aspx?id={id}&dbid=0`, `WebLink/Search.aspx`. (Several instances I probed were unreachable from this network, so the URL shapes are documented here from the product's known routes, not measured.)

**Design consequence:** Laserfiche portals are a **crawl target, not an API target**, and they are the hardest kind — document IDs are opaque integers, browse is paginated server-side, and the actual payloads are PDFs/TIFFs. `dbid` and an ID range are per-host configuration. Budget real effort here.

### 8.6 NovusAGENDA — **ASP.NET WebForms, no API, hostile to crawling**

Probed live at `https://tampa.novusagenda.com/`:

| URL | Result |
|---|---|
| `/agendapublic/` | 200, 1,384 bytes |
| `/agendapublic/meetingsresponsive.aspx` | 200, 1,453 bytes — **an error page**, `<form method="post" action="Error.aspx?handler=Application_Error+-Global.asax">` |
| `/agendapublic/MeetingView.aspx?MeetingID=1` | 200, 5,821 bytes, contains `__VIEWSTATE` **and** `__EVENTVALIDATION` |

`__VIEWSTATE` + `__EVENTVALIDATION` means **postback-driven navigation**: you cannot enumerate meetings with GETs alone; paging and filtering are `POST`s that echo back an opaque, single-use ViewState blob. Search entry is `Meetings.aspx`, detail is `MeetingView.aspx?MeetingID={int}`.

**Design consequence:** two viable strategies, both ugly — (a) integer-space enumeration of `MeetingID` with polite rate limiting, or (b) a headless browser to drive the postbacks (§7). This is the strongest argument in the survey for keeping browser rendering available as an *optional* capability rather than dropping it entirely.

### 8.7 CivicPlus (CivicEngage / Municipal Websites Central) — **API exists, but gated**

CivicPlus's own help documentation describes REST and SOAP APIs, reachable at `http://{site_name}/api`, and states that you must **"Contact CivicPlus Support to receive the API key and Token ID"** to unlock full access. There is a separate "Public Application Programming Interface (API)" article for their Agenda and Meeting Management product.
Sources: <https://www.civicplus.help/docs/application-programming-interface-api>, <https://www.civicengagecentral.civicplus.help/hc/en-us/articles/115004748093-APIs-and-Web-Central>, <https://www.meetingsessential.civicplus.help/hc/en-us/articles/10248707089559-Public-Application-Programming-Interface-API>

Live probes of `/api` on three candidate municipal sites returned 404, 403 and 404 — the endpoint is per-tenant, per-product, and evidently not enabled by default.

CivicPlus sites do commonly expose `RSSFeed.aspx?ModID={module}&CID={category}` feeds (news flash, calendar, bid postings). My probes returned 403 (`stpete.org` — the §2.1 WAF pattern) and 404/301 on other candidates, so **treat CivicPlus RSS as a per-host thing to discover and record, not a platform-wide guarantee.**

**Design consequence:** CivicPlus is a **crawl target with per-tenant feed opportunities**. Because CivicPlus is one of the most common municipal platforms in the US, this is where the bulk of ordinary HTML crawling will actually happen, and where §4's politeness work earns its keep.

### 8.8 Summary — API-first vs crawl-first

| Platform | Public machine-readable interface | Auth | Change signal | Verdict |
|---|---|---|---|---|
| **Legistar (Granicus)** | **Yes** — OData REST, 55 GET ops, `webapi.legistar.com/v1/{client}/…` | none for many clients (`?token=` for some) | **`*LastModifiedUtc` + `$filter`** | **API-first. Do not crawl the HTML.** |
| **Granicus ViewPublisher** | **Yes** — RSS 2.0 + `gran:` namespace | none | `<pubDate>`, stable `<guid>` | **Feed-first**; per-host `view_id` |
| **ArcGIS Hub** | **Yes** — Hub Search API (GeoJSON) + **DCAT-US 1.1** at `{site}/api/feed/dcat-us/1.1.json` | none | catalog `modified` dates | **Feed-first. Never HTML-crawl.** |
| **PrimeGov** | **Yes (undocumented)** — `/api/v2/PublicPortal/…`, `/api/meeting/search` | none | `publishDate` per document | API-first with HTML fallback |
| **Municode** | **Yes (undocumented)** — `api.municode.com` | none | — | API required (site is an SPA shell) |
| **CivicPlus** | Partial — per-tenant REST/SOAP behind a key; per-site `RSSFeed.aspx` | **key required** | RSS `pubDate` where available | **Crawl-first**, opportunistic feeds |
| **Laserfiche WebLink** | No anonymous API (Cloud API is OAuth-only) | OAuth (Cloud only) | — | **Crawl-first**, hard |
| **NovusAGENDA** | No | — | — | **Crawl-first**, needs POST/ViewState or a browser |

**Three conclusions that should shape Centinel's architecture:**

1. **Platform detection must be a first-class step in the crawl pipeline**, before generic HTML crawling. A per-host config already needs a UA/contact/rate policy (§2.2, §4.5); it should also carry a `platform:` discriminator and platform-specific parameters (`legistar_client`, `granicus_view_id`, `primegov_tenant`, `arcgis_hub_site`, `weblink_dbid`). Detection is cheap: hostname patterns (`*.legistar.com`, `*.granicus.com`, `*.primegov.com`, `*.novusagenda.com`, `*.opendata.arcgis.com`), plus probing the known API path once per host.

2. **The "collector" abstraction should not be "crawler."** It should be an interface with at least: `HtmlCrawlCollector`, `SitemapCollector`, `OdataCollector` (Legistar), `RssCollector` (Granicus, CivicPlus), `DcatCollector` (ArcGIS Hub), `JsonApiCollector` (PrimeGov, Municode). They all terminate in the same content-addressed store; only acquisition differs.

3. **Change tracking is not one mechanism.** Where an API exposes `LastModifiedUtc` (Legistar) or `publishDate` (PrimeGov) or DCAT `modified`, use it — it is authoritative, cheap, and precise. Fall back to markdown hashing only where nothing better exists. This materially reduces the re-crawl volume that §1.11 priced out as prohibitive on Firecrawl's credit model.

**None of this is language-dependent.** All six interface types are JSON or XML over HTTPS, and every language in this survey handles them trivially. That is itself a finding for §9: **the highest-leverage engineering in this project is not in the crawler, and therefore not in the language choice.**

## 9. What this means for the language decision

**This section does not pick a language.** It states what each choice actually buys and costs, using only findings established above.

### 9.1 The operator's prior was half right

The stated hypothesis was *"Rust, but the ecosystem is probably weaker."* Measured against the seven capability areas in this document:

| Capability | Rust | Python | TypeScript | Where Rust actually stands |
|---|---|---|---|---|
| Firecrawl SDK (§1.1) | **official, MIT, 2.12.1** | official | official | **prior was wrong** — the SDK exists and is current |
| Crawl framework (§3) | 1 (`spider`, very active) | 2 strong (Scrapy, crawlee) | 1 strong (crawlee) | thinner, but not absent |
| Latency-adaptive throttling (§4) | **yes** (`spider::auto_throttle`, EMA per domain) | **yes** (Scrapy AutoThrottle) | **no** | **parity with the best** |
| `Retry-After` end-to-end (§4) | **code exists, unwired** | **yes** (crawlee-python, best impl) | **no** | **behind Python** |
| robots.txt parser (§5) | good (`texting_robots`) | **best** (Protego: `Request-rate`) | good (`robots-parser`) | fine |
| Sitemap parser (§5) | **worst** — one crate, 2020, UTF-8 only; `sitemap-rs` can't parse | best (Scrapy + usp) | good (Crawlee, one bad default) | **prior was right** |
| HTML→markdown (§6) | **best serializer** (`htmd`, turndown parity **with tables**) | **best extractor** (`trafilatura`) | weakest (turndown needs GFM plugin) | **ahead of TS** |
| Boilerplate removal (§6) | good (`dom_smoothie`) | **best in survey** (`trafilatura`) | stale / WIP | behind Python |
| JS rendering (§7) | **no Playwright** — native CDP instead (`chromiumoxide`) | first-party Playwright | first-party Playwright | **different architecture, not worse** |
| Gov CMS APIs (§8) | n/a | n/a | n/a | **language-independent** |

**Two places the prior holds:** the sitemap layer (§5.1 — you will write it) and the `Retry-After` integration (§4.4 — the primitives exist, the wiring does not). Both are bounded, well-specified work: call it a few hundred lines total, against a spec that §2.4 and §4.6 already write for you.

**One place the prior is inverted:** HTML→markdown. `htmd` claims and tests turndown parity *including native table conversion* with `html5ever` as its only dependency, and `dom_smoothie` is a faithful `readability.js` port. TypeScript's default serializer cannot emit markdown tables without a plugin. For a corpus of budget tables and fee schedules (§6.4), that matters.

**One place it is simply wrong:** Firecrawl support — and more tellingly, **Firecrawl's own production crawl core is Rust** (§1.2), built on `texting_robots`, `lol_html`, `roxmltree`, `psl`. The leading commercial crawler already validated that these crates carry production load.

### 9.2 The findings that dominate the language question

Ranked by how much they should move the decision:

**1. Most of the hard work is not crawling (§8).** Three of the seven `.gov` platforms — Legistar, ArcGIS Hub, Granicus — expose keyless structured feeds, and two more (PrimeGov, Municode) expose undocumented JSON APIs. For those, the "crawler" is a typed HTTP client over JSON/XML, which every language does equally well. The crawler framework only earns its keep on CivicPlus, Laserfiche and generic Drupal/WordPress `.gov` sites. **The framework-count gap between Rust and Python shrinks in proportion to how much of the corpus is API-addressable.**

**2. The real blocker is a WAF 403, and no library solves it (§4.5).** It is fixed by a descriptive User-Agent, a contact address, and a per-host policy table — application config that must be written by hand in every language. `curl -A "Centinel Research …"` flipping `sec.gov` from 403 to 200 is the entire lesson. **This is the single highest-value component of the system and it is language-neutral.**

**3. Change tracking should be API-derived where possible (§8.8).** Legistar's `$filter=MatterLastModifiedUtc gt datetime'…'` is a server-side change feed. PrimeGov's `documentList[].publishDate` and DCAT's `modified` are the same idea. Markdown hashing is the *fallback*, not the primary mechanism. Again language-neutral, and again it reduces the weight of the crawler-framework question.

**4. Retention economics rule out hosted Firecrawl as the primary path (§1.11).** "Every version of every page, forever" against per-page credits — 1 credit/page, PDFs billed **per page**, no rollover, no pay-per-use — puts a 50k-page weekly re-crawl at ~2.6M credits/year before PDFs. Self-hosted Firecrawl loses change tracking, `/extract`, and Fire-engine (§1.10) and is AGPL. **So Centinel owns its crawler in some language, and Firecrawl becomes a selective tool (`/parse` for hard PDFs, `/map` for discovery deltas) rather than the engine.** That decision has to be made before the language one.

### 9.3 The honest tradeoffs

**Choosing Rust means:**
- ✅ One binary. No runtime, no venv, no `node_modules`. For a CLI + server + MCP that gets deployed and scheduled, this is a real operational win.
- ✅ Best-in-class markdown serialization (`htmd`, tables native), a credible Readability port (`dom_smoothie`), production-proven robots parsing (`texting_robots`), and the best rate-limiting primitives in the survey (`governor` GCRA, `Jitter::Full` by default in `retry-policies`, lock-free per-domain EMA in `spider`).
- ✅ **Compile-time optional browser support** (§7.3). `--features chrome` genuinely means zero footprint when off. Python and TS can only approximate this at runtime.
- ✅ Hash-addressed content stores, streaming, and large-corpus I/O are exactly Rust's strengths, and Centinel is at heart a storage system.
- ❌ **You write the sitemap layer** (§5.1). ~200–300 lines over `quick-xml`/`roxmltree` + `flate2` + magic-byte sniffing, to the §2.4 spec. The one existing parser crate is from 2020 and is UTF-8-only; `sitemap-rs` cannot parse at all.
- ❌ **You wire 429 → `Retry-After` → per-domain backoff yourself** (§4.4). `spider` ships `DomainRateLimiter::throttle(domain, retry_after)` and `AdaptiveConcurrency` as *unwired public utilities*. ~150 lines of glue.
- ❌ **One framework, one maintainer-org.** `spider` is excellent and hot (0 open issues, pushed daily) but it is a single point of ecosystem failure, and it is feature-flag-maximalist in a way that will cost you build-config time.
- ❌ **No Playwright.** `chromiumoxide` is good and pure-Rust but you supply the browser and you get CDP, not Playwright's ergonomics or its auto-waiting. For §8.6's NovusAGENDA ViewState postbacks this is the least pleasant option.
- ❌ Slowest iteration loop of the three for exploratory work — and §8 shows this project involves a lot of "probe an undocumented API and see what comes back."

**Choosing Python means:**
- ✅ **Scrapy is the most complete crawling system in existence** (§3.2, §4.1, §5.2): AutoThrottle with the `status != 200` guard nothing else has, per-domain slots, `SitemapSpider` with magic-byte gzip sniffing and `recover=True` XML parsing, `sitemap_filter()` for `lastmod`-based skipping, and pluggable everything.
- ✅ **`trafilatura` is the best content extractor in the survey**, with published benchmarks and institutional adoption, emitting markdown in one step.
- ✅ **`crawlee-python` has the only correct `Retry-After` implementation found** (§4.2), including HTTP-date form and negative/past-date rejection.
- ✅ `Protego` is the only robots parser that reads `Request-rate` — the one directive that maps onto a token bucket.
- ✅ Fastest iteration for probing undocumented `.gov` APIs (§8.3, §8.4).
- ❌ **Two frameworks, and you must choose:** Scrapy has the adaptive throttle and the sitemap engine but no `Retry-After`; crawlee-python has `Retry-After` and per-domain backoff but no latency adaptation. Neither is a superset.
- ❌ **Licence hazards are real here.** `usp` is **GPL-3.0**, `html2text` is **GPL-3.0-or-later**. For a shipped library both must be avoided; permissive substitutes exist (Scrapy's own sitemap code, `markdownify`), so this is a discipline problem, not a blocker.
- ❌ **Deployment.** Interpreter + venv + a compiled extension (`lxml`, `selectolax`) + optional Playwright browsers, for a tool meant to run as a CLI, a server, and an MCP. This is the mirror image of Rust's single-binary win.
- ❌ Scrapy is Twisted-based; `httpx` (the assemble-your-own fetcher) has not shipped since **2024-12-06** and is still pre-1.0.
- ❌ **Dangerous defaults you must remember:** `ROBOTSTXT_OBEY = False`, `AUTOTHROTTLE_ENABLED = False`, `trafilatura(links=False)`, and crawlee-python's throttling being opt-in with an explicit domain list.

**Choosing TypeScript means:**
- ✅ **Crawlee is the best-architected framework of the three** for what Centinel does: one `RequestQueue`, one storage abstraction, and HTTP-vs-browser as a class swap. `enqueueLinks({ strategy: 'same-hostname' })` is precisely the `.gov` scoping primitive.
- ✅ Crawlee's sitemap fetcher is well engineered — magic-byte gzip sniffing, `whatwg-mimetype` `+xml` detection, unbounded depth with cycle protection.
- ✅ First-party Playwright and Puppeteer; the best story for §8.6's postback-driven portals.
- ✅ `got-scraping` (header ordering, TLS fingerprint) has no equal in Rust or Python — though §4.5 argues Centinel should *not* want it.
- ✅ Shares a language with a web UI if one is ever added (currently out of scope — "No UI").
- ❌ **Weakest rate-limit story of the three** (§4.3). Crawlee's autoscaling adapts to *your machine and the Apify API*, not the target host — `clientInfo` reads `config.getStorageClient().stats.rateLimitErrors`. Zero `Retry-After` handling in the repo. Its answer to blocking is **session/identity rotation** (`BLOCKED_STATUS_CODES = [401, 403, 429]`), which is the wrong ethic for a transparency crawler that intends to identify itself.
- ❌ **A default that silently breaks a documented `.gov` case** (§5.3): `RobotsTxtFile.getSitemaps()` defaults to `enqueueStrategy: 'same-hostname'` and therefore discards `hillsboroughcounty.org` → `hcfl.gov`. One option fixes it; you have to know.
- ❌ **Weakest markdown story** (§6.3): turndown needs a GFM plugin for tables; `@mozilla/readability` has not released since 2025-03-03; `defuddle` self-describes as *"very much a work in progress."*
- ❌ Sax strict-mode XML parsing destroys the stream on malformed input, where Scrapy recovers — bad trade for municipal sitemaps.
- ❌ **Hardest language to make browser support genuinely optional** (§7.3), and `puppeteer` (vs `puppeteer-core`) downloads a browser in postinstall.

### 9.4 What would actually decide it

Questions whose answers move this more than any library comparison:

1. **What fraction of the target corpus is API-addressable (§8)?** If most jurisdictions of interest run Legistar / ArcGIS Hub / PrimeGov, the crawler framework barely matters and the decision collapses to deployment ergonomics — which favours Rust's single binary. If the corpus is mostly CivicPlus and bespoke Drupal, the framework matters a lot and Scrapy's maturity is hard to argue with.

2. **Is the operator willing to write ~400–500 lines of sitemap + backoff glue in Rust?** That is the measured size of the Rust gap (§5.1, §4.4). It is bounded and spec'd. If yes, Rust's disadvantages are largely paid off up front. If no, Python hands it to you today.

3. **Does the MCP surface need to be a subprocess or a library?** A Rust binary that also serves MCP is one artifact. A Python or TS MCP server is a runtime + dependency tree. This is an operational question, not a technical one, and it is the strongest single argument for Rust.

4. **How much PDF/OCR work is there really?** §1.8 shows Firecrawl handles PDFs, DOCX, XLSX well but bills per page. §6.1 shows `spider_transformations` bundles `calamine` + `zip` + `quick-xml` for Office formats in Rust. Python's PDF ecosystem is broader. If document extraction turns out to be the bulk of the work rather than HTML crawling, that shifts the balance toward Python.

5. **Is a mixed architecture acceptable?** Nothing forces one language. A Rust core (fetch, store, hash, diff) with a Python sidecar for extraction (`trafilatura`) is defensible — but it forfeits the single-binary property that is Rust's main advantage here, so it is probably the worst of both unless extraction quality proves decisive.

### 9.5 One thing that is true regardless

The components that will determine whether Centinel works are, in order:

1. the **per-host policy table** — UA, contact, rate, platform discriminator, known-403 remediation (§2.2, §4.5, §8.8)
2. **platform detection and per-platform collectors** (§8.8)
3. **API-native change signals** where they exist, markdown hashing where they do not (§8.8)
4. the **content-addressed store** and version retention

None of those four is a library. All four are yours to write in whichever language is chosen, and they are where the project's actual difficulty lives. The library survey above should be read as "what does each language save me on the remaining 30%," not as the decision itself.
