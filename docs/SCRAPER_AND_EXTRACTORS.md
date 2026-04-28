---
title: Tampa-DOGE — Scraper & Extractors (LOCKED, with deferred details)
status: 🔒 Interface locked v1 — extractor catalog deferred to post-spike
created: 2026-04-26
parent: README.md
---

# Scraper & Extractors

## Decision (revised 2026-04-26)

**Use Hermes' built-in web tools first** (`web_extract`, `web_search`, browser tools). They already cover HTML→markdown, PDF→markdown, JS rendering, and screenshots — there is no need for a custom Scraper layer in v0.1.

Add thin third-party adapters under `lib/adapters/` only if the spike surfaces concrete gaps Hermes can't fill (most likely: bulk domain-mapping, where Firecrawl `/map` may beat naive crawl). Adapter API keys live in operator's `.env`; absent keys disable the adapter, never break the system.

The earlier "Scraper interface" abstraction below is preserved as reference design in case Hermes' tools fall short on a specific document type, but is NOT shipped in v0.1.

---

## What's locked

### 1. Scraper interface

Every fetch goes through this interface. Implementations are swappable.

```python
# tampa_doge/scraper.py
from typing import Protocol, Iterator

class Scraper(Protocol):
    def scrape(self, url: str, *, render_js: bool = True) -> ScrapeResult:
        """Returns markdown + metadata + screenshot + raw_html."""

    def map(self, domain: str, *, limit: int = 5000) -> list[str]:
        """Discover all URLs under domain."""

    def crawl(self, root: str, *, depth: int, exclude: list[str]) -> Iterator[ScrapeResult]:
        """Walk from root, yield results as found."""

class ScrapeResult:
    url: str
    final_url: str           # after redirects
    markdown: str            # parsed content as markdown
    raw_html: str | None
    screenshot_bytes: bytes | None
    content_type: str
    fetched_at: datetime
    sha256: str              # hash of raw bytes
    scraper: str             # which impl produced this
    cost_estimate_usd: float | None
```

Operator picks one impl in config. v0.1 supports:
- `firecrawl` (default — universal HTML + PDF → markdown, has `/map` and `/crawl`)
- `tavily` (alternative)
- `playwright_local` (free, self-hosted fallback)

**Why the abstraction is mandatory even with one impl:** when Firecrawl prices change or shuts down or a fork wants Tavily, we change one config line. Cost of the abstraction is ~50 lines of code; cost of NOT having it is rewriting 5 skills.

### 2. Privacy rule

Third-party scrapers see **only public URLs we're fetching from `.gov`**. They NEVER receive:

- Operator queue files
- Draft findings
- Wiki entity pages (post-synthesis content)
- DB queries or DB contents
- Vault sidecars (parsed extractions live here)
- Anything in `<wiki>/_runtime/`
- Source ledger or any source-protection artifact (these don't exist in agent space anyway)

Locked as a hard rule. Implementation: scraper interface only takes URLs as input, never local file content. The extraction step (markdown → structured data) happens in our own LLM calls or local code, NOT through Firecrawl `/extract`.

### 3. Extraction split

```
url
 ↓ Scraper.scrape()           ← third-party (Firecrawl)
markdown + screenshot
 ↓ Extractor.classify()       ← OUR code, OUR LLM call
extractor schema name
 ↓ Extractor.extract()        ← OUR code, OUR LLM call (with markdown + schema)
structured data
 ↓ data_reporter.upsert()
DB rows
```

The third party fetches and renders. We extract structure. This keeps us in control of the synthesis layer (where civic-data sensitivity actually lives).

### 4. Cost discipline

Firecrawl is per-page priced (~$0.005–0.015/page). Rough Tampa budget:

| Activity | Volume | Cost/year |
|---|---|---|
| Initial bootstrap | 5000 pages | $50 one-time |
| Weekly lint (cheap mode) | 5000 × 52 × $0.003 | $780 |
| Investigations (5 active) | 50 pages/week × 52 × $0.01 | $130 per investigation = $650 |
| PDFs | 100/month × $0.02 | $288 |
| **Total** | | **~$1500–2000/year per city** |

Operator pays their own Firecrawl bill (their own API key, set in config). Abstraction lets us mix scrapers later (e.g., Playwright for hash-check polling, Firecrawl only when content changes) for 5–10x cost reduction.

## What's deferred to post-spike

### Extractor catalog

The schema-per-document-type layer (`meeting`, `rfp`, `award`, `budget-line`, `permit`, etc.) is **not designed yet**. Reason: we don't know what document shapes tampa.gov actually emits until we crawl it. Designing extractors before the spike is speculation; designing them after is informed.

**Plan:** the sitemap-builder spike runs Firecrawl against tampa.gov, dumps markdown for a representative sample, and we cluster what we see. The extractor catalog emerges from that clustering, not from a vendor-features-list.

### Browsing tool comparison

The spike should try at least two:
- **Firecrawl** (`/scrape`, `/map`, `/crawl`, `/extract` — see what each gives us)
- **Tavily** (compare quality, cost, JS rendering, PDF handling)
- **Playwright local** (baseline for what's free)

Decision criteria from the spike:
- Markdown quality on Granicus / Legistar / OpenGov pages (these are the JS-heavy SPAs that break naive crawlers)
- PDF→markdown quality on Tampa's actual budget book + award PDFs
- Map endpoint coverage vs. naive crawl
- Cost per representative day
- Reliability (rate limits hit, timeouts, errors)

Output: `~/plans/tampa-doge/research/scraper-comparison.md` after the spike.

### Fallback chain

Locked AFTER the spike when we know what the failure modes look like. Sketch:

```
Scraper.scrape(url)
   ↓ if fails or returns suspiciously empty markdown
Scraper.scrape(url, render_js=true, force_screenshot=true)
   ↓ if still fails
LocalPlaywrightScraper().scrape(url)
   ↓ if still fails
mark sitemap entry status: broken, log, move on
```

But the actual thresholds (what counts as "suspiciously empty") need real data.

## What this changes in the five skill specs

These edits are pending — will patch after the spike validates the approach:

- **sitemap-builder**: "Playwright + httpx + BeautifulSoup" → "Scraper.map() + Scraper.scrape()". Description-pass prompt no longer suggests parsers; instead suggests an extractor schema name (deferred until catalog exists; for now, suggests `generic`).
- **civic-investigator**: "Parser registry" references → "Extractor pipeline" references. Extraction call moves from `parser.extract()` to `Extractor.classify() → Extractor.extract()`.
- **civic-archivist**: "pdfplumber + ocrmypdf + tesseract" → "Scraper.scrape() returns markdown directly for PDFs". Vault still stores the raw PDF bytes (immutability rule); the markdown sidecar comes from Scraper.
- **civic-data-reporter**: no change — it consumes structured data, doesn't care where it came from.
- **civic-watch-runner**: no change — queries DB rows.

## Acceptance criteria for the spike (becomes acceptance for this design)

The spike validates this design if and only if:

- ✅ Firecrawl renders Granicus + Legistar SPA pages with usable markdown (not "enable JavaScript")
- ✅ Firecrawl converts a Tampa budget book PDF to readable markdown without manual OCR setup
- ✅ Firecrawl `/map` produces a more complete URL list than a naive crawl
- ✅ Total bootstrap cost for tampa.gov is under $100
- ✅ The Scraper abstraction can be implemented in <100 lines with both Firecrawl and Playwright behind it
- ✅ Markdown output is uniform enough across page types that one extractor approach (LLM with schema) works on at least 3 distinct page shapes (meeting agenda, RFP, contract award)

If any of these fail, we revisit — possibly back to local Playwright + custom extractors for some content types.

## Open questions (resolve at or after spike)

1. Does Firecrawl `/extract` (their schema-based structured extraction) produce good enough output that we can use it instead of our own LLM extraction layer? If yes, **the privacy rule may need to soften** for non-sensitive content — or we don't use it. Default: don't use it; keep extraction in our LLM space.
2. Should we cache scrape results locally to avoid re-paying for unchanged pages? The vault already does this via sha256 dedup — verify the abstraction surfaces "cached, $0" results properly so cost telemetry is honest.
3. Browser-tool comparison output should also note: which gives us screenshots? (Some operators will want screenshot-as-evidence in the vault for journalistic integrity. Firecrawl does; not sure about Tavily.)
4. Rate limits on weekly lint — 5000 pages in one cron run may hit Firecrawl rate limits. May need to spread the lint over the week (different sections on different days).
