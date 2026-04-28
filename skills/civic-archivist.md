---
title: civic-archivist (skill spec)
status: 🧠 Specced
created: 2026-04-26
agent_role: Archivist
parent: ../README.md
---

# `civic-archivist` — Skill Spec

## Purpose

Document intake. Every external resource (PDF, HTML page, transcript, image) the system encounters gets hashed → vaulted → OCR'd → indexed → tagged → summarized. Maintains vault manifest integrity. The unglamorous backbone of the whole system. Maps to the Spotlight 2nd-seat Reporter who reads 50–200 pages a day.

## When this skill activates

- Called inline by `civic-investigator` when it encounters a resource
- Called inline by `sitemap-builder` for non-HTML linked documents (PDFs)
- Operator drops a file in `<wiki>/Vault/_inbox/` for manual ingest
- Vault manifest cron (nightly) for integrity checks and orphan detection

## Inputs

```yaml
mode: vault | reingest | manifest_check | manual_inbox
target:
  url: https://...                        # vault mode: a URL to fetch and vault
  vault_path: Vault/pdfs/...              # reingest mode: re-OCR an existing vault entry
  inbox_dir: Vault/_inbox                 # manual_inbox mode
parser_hint: contracts-portal-tampa | null  # from sitemap entry
caller_context:                           # optional, sets `discovered_via`
  investigation: parks-contractors
  source_url: https://...
  link_text: "FY2025 Parks Capital Awards (PDF)"
```

## Outputs

1. **Vault file** at `<wiki>/Vault/<type>/<YYYY-MM-DD>-<sha8>-<slug>.<ext>` (immutable original)
2. **Parsed sidecar** at same path with `.md` extension for parsed text
3. **Manifest entry** appended to `<wiki>/Vault/manifest.jsonl`
4. **Sitemap entry update** — content_hash for the URL bumped; `linked_entities` updated if Archivist extracts entity hints
5. **Source summary page** at `<wiki>/Sources/<slug>.md` (the 1–3 paragraph "what is this document" summary the Spotlight 2nd-seat would file)

## Vault layout (recap)

```
<wiki>/Vault/
├── pdfs/<YYYY-MM-DD>-<sha8>-<slug>.pdf      # original, never modified
├── pdfs/<YYYY-MM-DD>-<sha8>-<slug>.md       # parsed markdown sidecar
├── html/<YYYY-MM-DD>-<sha8>-<slug>.html     # raw HTML capture
├── html/<YYYY-MM-DD>-<sha8>-<slug>.md       # parsed markdown sidecar
├── transcripts/<YYYY-MM-DD>-<meeting-id>.txt
├── images/<YYYY-MM-DD>-<sha8>-<slug>.png
├── _inbox/                                   # operator manual drops, processed and moved
└── manifest.jsonl
```

## Manifest entry schema

One line per vaulted document, append-only:

```json
{
  "vault_path": "pdfs/2026-04-26-a1b2c3d4-fy2025-parks-awards.pdf",
  "url": "https://www.tampa.gov/sites/default/files/parks-awards-fy2025.pdf",
  "sha256": "a1b2c3d4...",
  "fetched_at": "2026-04-26T14:32:11-04:00",
  "type": "pdf",
  "size_bytes": 482113,
  "parser": "tampa-budget-pdf",
  "ocr_engine": "tesseract-5.3",
  "page_count": 24,
  "summary_path": "Sources/2026-04-26-fy2025-parks-awards.md",
  "sitemap_entry": "https://www.tampa.gov/procurement/awards",
  "discovered_via": {
    "investigation": "parks-contractors",
    "caller_url": "https://www.tampa.gov/procurement/awards",
    "link_text": "FY2025 Parks Capital Awards (PDF)"
  },
  "tags": ["budget", "parks", "fy2025"],
  "entity_hints": ["ACME Construction LLC", "Tampa Parks Department"]
}
```

## Algorithm

```
on_vault(url):
  1. HEAD url; if mime in (image/, video/), bail → not vaultable text
  2. GET url with respectful UA + rate limit
  3. compute sha256 of body
  4. if manifest contains sha256 (deduped):
       reuse existing vault_path; just record discovered_via
       return existing manifest row
  5. choose vault subdir from content-type (pdf/html/transcripts/images)
  6. write original to vault_path (atomic temp-then-rename)
  7. parse to markdown:
       - PDF: try pdfplumber → if image-only, fallback to ocrmypdf + tesseract
       - HTML: trafilatura or readability + html2text
       - audio/video transcripts: not in this skill (separate transcript pipeline)
  8. write parsed sidecar
  9. LLM summary pass (1-3 paragraphs):
       - what is this document
       - who created it (department, vendor, official)
       - what dates it covers
       - what types of facts it asserts (claims, financial figures, names)
       - what would be needed to corroborate
       output goes to Sources/<slug>.md with frontmatter
 10. LLM entity-hint pass:
       - lightweight named-entity extraction (people, orgs, contractors, dollar amounts)
       - hints only — civic-data-reporter does the canonical entity reconciliation
 11. append manifest line (atomic)
 12. log to <wiki>/log.md
```

## Source summary page template

`<wiki>/Sources/2026-04-26-fy2025-parks-awards.md`:

```yaml
---
title: FY2025 Parks Capital Awards
type: source
created: 2026-04-26
vault_path: Vault/pdfs/2026-04-26-a1b2c3d4-fy2025-parks-awards.pdf
sha256: a1b2c3d4...
url: https://www.tampa.gov/sites/default/files/parks-awards-fy2025.pdf
discovered_via: parks-contractors
tags: [budget, parks, fy2025]
---

# FY2025 Parks Capital Awards

**Document:** [[Vault/pdfs/2026-04-26-a1b2c3d4-fy2025-parks-awards|fy2025-parks-awards.pdf]] (24 pages, 471 KB)
**Source URL:** https://www.tampa.gov/sites/default/files/parks-awards-fy2025.pdf
**Discovered via:** investigation [[parks-contractors]]

## What this is
[1-3 paragraph LLM summary]

## Entity hints (for downstream reconciliation)
- ACME Construction LLC
- Tampa Parks Department
- ...

## Corroboration needed
- These award amounts should be matched against City Council vote records for FY2025 budget approvals.
- Vendor names should be checked against SunBiz registrations.
```

## Manifest cron (nightly)

1. Walk `<wiki>/Vault/` directory tree.
2. For each file: compute sha256, look up in manifest.
3. **Orphan vault file** (file exists, no manifest entry): emit warning, do not touch.
4. **Orphan manifest entry** (manifest entry, no file): emit error.
5. **Hash mismatch** (file content sha256 differs from manifest): emit critical alert — vault was modified, which violates immutability.
6. Append manifest health report to `<wiki>/Vault/_health-<YYYY-MM-DD>.md`.

## Pitfalls

- **PDF OCR is slow and expensive.** Hundreds-of-pages budget books take minutes. Run OCR in a background queue, not synchronously inside `civic-investigator`'s main loop. Investigator should be allowed to proceed with the parsed-where-possible text and let OCR catch up async.
- **Same content, different URL.** Cities re-host the same PDF at three URLs (sites/default/files, /procurement/uploads, attached to council agenda). sha256 dedup catches this — record all three URLs in `discovered_via`.
- **Content drift.** A URL hosting a "live" PDF that gets updated quarterly will fail the immutability rule if you try to overwrite. **Each fetch with a different sha256 = a new vault entry.** The manifest preserves the chain.
- **Atomic writes.** A crashed write must not leave a half-written PDF in vault. Write to `_tmp/` then rename.
- **Mime sniffing lies.** Server says `text/html`, body is actually a PDF. Always sniff the magic bytes.
- **JS-rendered HTML.** Same as sitemap-builder — Playwright fallback. Vault both the rendered HTML and a screenshot for evidence integrity.
- **Don't put OCR'd text in the wiki directly.** Wiki entity pages cite the vault path. The OCR sidecar is for the agent's own search and the qmd index — not human reading.
- **Inbox abandonment.** Operator drops files, agent processes, but if the agent crashes the inbox stays full. Always drain `_inbox/` to a `_inbox/_processed/<date>/` folder rather than deleting after vault.

## Dependencies

- `pdfplumber`, `ocrmypdf`, `tesseract`
- `trafilatura` or `readability-lxml`, `html2text`
- `playwright` (JS-rendered HTML)
- `python-magic` (mime sniffing)
- `obsidian` skill for source-page wikilink hygiene
- LLM call for summary + entity-hint passes

## Verification (acceptance criteria)

- ✅ Vaulting a PDF the second time (same content, different URL) produces no new file but records the new URL in `discovered_via`
- ✅ Image-only scanned PDF gets OCR'd; the markdown sidecar contains readable text
- ✅ JS-rendered SPA page yields rendered HTML + screenshot in vault
- ✅ Manifest cron detects a manually deleted vault file as orphan_manifest
- ✅ Manifest cron detects a manually edited PDF (sha mismatch) as critical alert
- ✅ Source summary page is browsable in Obsidian, links to vault file via wikilink
- ✅ Two simultaneous Investigators vaulting the same URL race-safely (one wins, both get the same manifest row)

## Open questions (for the operator)

1. Should images (screenshots, scanned receipts) get OCR + summary like PDFs, or just be archived without parse? Default proposal: yes, OCR + brief caption summary.
2. Audio/video files from Granicus meetings — vault the audio? transcribe with Whisper here? or hand off to a separate `transcript` pipeline? Default proposal: separate pipeline; Archivist only handles text-bearing documents.
3. How long do we keep `_inbox/_processed/` snapshots before pruning? Default proposal: 90 days.
