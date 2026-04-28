---
title: sitemap-builder (skill spec)
status: 🧠 Specced
created: 2026-04-26
agent_role: Cartographer
parent: ../README.md
---

# `sitemap-builder` — Skill Spec

## Purpose

Build and maintain a labeled sitemap of a city's `.gov` web surface. The sitemap is Tampa-DOGE's central artifact — the human's browsing entrypoint and the launching pad for every investigation. This skill produces it and keeps it fresh.

## When this skill activates

- Operator runs `bootstrap` against a new city.gov for the first time
- Weekly cron (`sitemap-lint`) fires
- Operator manually requests a re-crawl of a subtree
- An investigation discovers URLs not yet in the sitemap (skill called inline to register them)

## Inputs

```yaml
# Invocation contract
mode: bootstrap | lint | subtree | register
target:
  domain: www.tampa.gov                  # for bootstrap and lint
  subtree_url: https://...                # for subtree mode
  urls: [list]                            # for register mode
config:
  wiki_path: ~/wiki/Tampa
  max_depth: 5                            # crawl depth from sitemap.xml seeds
  max_pages: 5000                         # safety cap
  respect_robots: true
  user_agent: TampaDOGE/0.1 (+contact)
  rate_limit_rps: 1                       # be polite
  exclude_patterns:
    - /search?
    - .pdf$            # PDFs go to vault via Archivist, not sitemap
    - /calendar/print
```

## Outputs

1. `<wiki>/Sitemap/index.md` — human-readable, grouped by section
2. `<wiki>/Sitemap/sitemap.json` — machine-readable, full schema
3. `<wiki>/Sitemap/log.md` — append-only diff log
4. `<wiki>/Sitemap/_lint-report-<date>.md` — lint mode only
5. New entries flagged `status: needs_review` for operator approval

## Sitemap entry schema

```yaml
- url: https://www.tampa.gov/procurement/awards
  type: contracts
  description: |
    Page lists awarded city contracts grouped by department, updated quarterly.
    Each row links to a contract PDF.
  content_kind: index
  contains:
    - awarded contracts (table)
    - PDF links to award docs
  linked_entities: []
  last_crawled: 2026-04-25
  content_hash: sha256:abc...
  parser: contracts-portal-tampa
  crawl_freq: weekly
  status: active                          # active | broken | excluded | needs_review
  notes: []
```

### Allowed `type` values

`meetings | contracts | rfps | budget | boards | permits | ethics | press | personnel | project | document | profile | calendar | form | general`

### Allowed `content_kind` values

`index | document | listing | form | profile | news | calendar | search`

## Modes

### `bootstrap` (one-time per city)

1. Fetch `domain/sitemap.xml` and any `sitemap-index.xml`. Parse all referenced sub-sitemaps.
2. Recursively crawl from seeds (sitemap.xml URLs + homepage), respecting `max_depth`, `max_pages`, `exclude_patterns`, `robots.txt`, `rate_limit_rps`.
3. For each URL discovered:
   a. HEAD request → check content-type
   b. If HTML: GET, store body for description pass
   c. If PDF/binary: skip (will be vaulted by Archivist when investigations encounter it)
4. Compute `content_hash` (sha256 of normalized body — strip session tokens, csrf, dates from headers).
5. **Description pass:** for each new URL, LLM call with the body and a few-shot prompt → `description`, `type`, `content_kind`, `contains`, suggested `parser`.
6. Emit sitemap.json + sitemap.md.
7. Mark every entry `status: needs_review` for first-time bootstrap.
8. Post bootstrap report: total URLs, by type, sample of 5 needs_review entries.

### `lint` (weekly cron)

1. Re-fetch known URLs (HEAD for cheap-changed-check; GET if hash mismatch).
2. Re-crawl recently-changed sections + crawl 1 hop further from any URL added since last lint.
3. For each URL in the sitemap:
   - Still reachable (200)? If not → `status: broken`, log
   - `content_hash` changed? → re-run description pass, bump `last_crawled`
   - URL pattern matches an `exclude_patterns` rule added since last lint? → `status: excluded`
4. For each newly discovered URL:
   - Add as `status: needs_review` with description pass output
5. Append diff to `Sitemap/log.md`:
   - `+ added: N`, `- removed: M`, `~ changed: K`, `! broken: J`, `? needs_review: P`
6. Write lint report to `Sitemap/_lint-report-<YYYY-MM-DD>.md` with full per-URL changes.
7. Post compact summary to operator's notification channel (one-line: "Sitemap lint: +3 new, 1 broken, 5 changed. See report.").

### `subtree` (manual operator trigger)

Operator points at a URL ("re-crawl /procurement/"). Same as bootstrap but rooted at that URL with depth scoped from there.

### `register` (called inline by `civic-investigator`)

Investigator finds URLs during a depth crawl. Calls this skill with `urls: [...]`. Each URL gets a description pass and is added to sitemap with `status: needs_review`.

## Description-pass prompt (sketch)

```
You are cataloging a page on a city's .gov website for an investigative civic-data agent.
Given the URL and HTML body, output JSON with:
  - description (2-3 sentences: what is this page, what data does it expose)
  - type (one of: meetings, contracts, rfps, budget, boards, permits, ethics, press,
    personnel, project, document, profile, calendar, form, general)
  - content_kind (one of: index, document, listing, form, profile, news, calendar, search)
  - contains (list of 1-5 short phrases describing what's on the page)
  - parser_suggestion (string, or null if no specific parser applies)

Rules:
- Be concrete. "Lists 47 awarded contracts as of Q2 2026" beats "lists contracts".
- If the page is a search/calendar/form with no static content, say so and recommend
  status: excluded.
- If the page links to a known portal vendor (Granicus, Legistar, CivicPlus, OpenGov,
  Bonfire, eTRAKiT, Accela, NovusAGENDA), suggest the matching parser.
```

## Parser registry interaction

Each sitemap entry carries a `parser` field. The parser is a Python module under `parsers/<name>.py` in the Tampa-DOGE repo. Sitemap-builder does NOT call parsers — it only suggests them. `civic-investigator` and `civic-archivist` consume the parser hints when they ingest specific pages.

## Pitfalls

- **JS-rendered content.** Many `.gov` portals (Granicus video, OpenGov dashboards) are SPAs — `wget` returns an empty shell. Skill must use a headless browser (Playwright) for JS-rendered pages. Detect: empty `<body>` or `<noscript>` tag with "enable JavaScript" text.
- **Calendar / search infinite loops.** `?date=2099-12-31` style URLs are unbounded. Always exclude calendar and search query URLs by default.
- **Session tokens in URLs.** Some portals embed jsessionid / csrf token in the path. Normalize before hashing.
- **PDF links inflate sitemap.** PDFs belong in the Vault, not the sitemap. The sitemap entry for the *index page* says "links to N PDFs"; the PDFs themselves are vaulted on-demand.
- **Over-deep crawls.** A city site can have 100K+ URLs. Hard cap at `max_pages` and report what was skipped.
- **`needs_review` backlog.** Without operator review, the sitemap rots. Lint report must surface the count of unreviewed entries prominently.
- **robots.txt vs. journalism.** Default to respecting robots.txt. Operator can override per-domain in config — that's a human policy decision, not the agent's.

## Dependencies

- `playwright` (headless Chromium for JS-rendered pages)
- `httpx` (cheap HEAD/GET for static pages)
- `beautifulsoup4` + `lxml` (HTML parsing)
- `tldextract` (domain handling)
- LLM call for description pass

## Verification (acceptance criteria)

- ✅ Bootstrap against tampa.gov produces a sitemap.json with 500–5000 entries
- ✅ Each entry has all required fields populated
- ✅ Sitemap.md is browsable in Obsidian, grouped by `type`
- ✅ Lint run after a known new page is added detects it and flags `needs_review`
- ✅ Lint run after a known page is taken down flags it `broken`
- ✅ JS-rendered Granicus meeting page yields a meaningful description, not "enable JavaScript"
- ✅ Excluded patterns produce zero crawled URLs

## Open questions (for the operator)

1. Should bootstrap require interactive operator approval before writing 1000+ entries, or auto-write everything as `needs_review`?
2. What's the right `crawl_freq` default — weekly for everything, or per-`type`? (Press releases probably daily; budget probably monthly.)
3. Tampa-specific exclude_patterns will only emerge after the first real crawl. Plan to iterate.
