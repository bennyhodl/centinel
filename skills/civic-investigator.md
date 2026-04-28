---
title: civic-investigator (skill spec)
status: 🧠 Specced
created: 2026-04-26
agent_role: Investigator
parent: ../README.md
---

# `civic-investigator` — Skill Spec

## Purpose

Run an operator-defined investigation end-to-end: depth-crawl from seed URLs, extract entities into wiki pages, accumulate evidence in the investigation file, and drop candidate connection findings into `Findings/draft/` for human review. Maps to the Spotlight Lead Reporter — owns the principal source relationships and the central narrative spine of one investigation.

## When this skill activates

- Operator creates or edits an investigation file in `<wiki>/Investigations/`
- Per-investigation cron schedule fires (`schedule: daily | weekly | monthly`)
- Operator manually triggers `re-run <investigation-name>`

## Inputs

The investigation YAML file is the input. Schema:

```yaml
---
title: Parks contractors over the last 5 years
goal: |
  Identify every contractor that has received parks-department funding,
  cumulative $$ awarded, and any cross-department repeat awards.
seeds:
  - https://www.tampa.gov/parks
  - https://www.tampa.gov/procurement/awards
status: active                  # active | paused | done | archived
depth: 3                        # max hops from seeds
schedule: weekly
date_range:                     # optional: scope the investigation in time
  from: 2021-01-01
  to: null                      # null = present
focus_entities: []              # optional: bias extraction toward these
exclude_urls:                   # optional: paths to skip
  - /calendar/
created: 2026-04-25
updated: 2026-04-25
---

## Notes
[operator's running notes; agent appends to "## Run log" below; never edits notes]

## Run log
[append-only, agent owns]
```

## Outputs

For each run:

1. **Wiki entity pages** created/updated in `<wiki>/Contractors/`, `<wiki>/People/`, `<wiki>/Projects/`, `<wiki>/Funding/`, `<wiki>/RFPs/`, `<wiki>/Orgs/`, `<wiki>/Boards/`.
2. **Vault entries** for every PDF/HTML/transcript encountered (delegated to `civic-archivist`).
3. **Database rows** inserted for every entity / relationship / transaction (delegated to `civic-data-reporter`).
4. **Investigation page run-log update** — append a `### Run YYYY-MM-DD HH:MM` section with: pages crawled, entities found (new vs. existing), files vaulted, candidate findings produced, blockers.
5. **Candidate findings** in `<wiki>/Findings/draft/` for any narrative connection (contractor↔donor, vote↔funding, etc.). Hard data findings go to `<wiki>/Findings/raw/` directly.
6. **Sitemap registration** — any URL crawled that wasn't in the sitemap is registered via `sitemap-builder` in `register` mode.

## Algorithm

```
on_run(investigation_path):
  1. parse investigation YAML
  2. if status != active: skip with note
  3. resolve sitemap entries for each seed URL (or register if missing)
  4. frontier = seeds; visited = set(); depth_map = {seed: 0}
  5. while frontier and len(visited) < max_pages:
       url = frontier.pop()
       if url in visited or url matches exclude_urls: continue
       if depth_map[url] > investigation.depth: continue
       page = fetch(url)                       # via Archivist for vaulting
       sitemap_entry = lookup_or_register(url)
       parser = registry.get(sitemap_entry.parser) or generic_extractor
       extracted = parser.extract(page, focus=goal, focus_entities=focus_entities)
       for entity in extracted.entities:
         upsert_wiki_page(entity)              # creates or updates
         data_reporter.upsert_entity(entity)   # database row
       for transaction in extracted.transactions:
         data_reporter.upsert_transaction(transaction)
       for link in extracted.outbound_links:
         if same_domain(link) and depth_map[url] + 1 <= depth:
           depth_map[link] = depth_map[url] + 1
           frontier.append(link)
       visited.add(url)
  6. analysis pass:
       - run goal-shaped queries against the database
         (e.g. for "parks contractors": SELECT contractor, SUM(amount)
          FROM transactions WHERE department='Parks' GROUP BY contractor)
       - LLM synthesis: read entity pages relevant to goal,
         draft "what we know now" summary
       - LLM connection-finding pass: scan for cross-references
         that look like notable patterns (donor↔award, official↔vendor relative,
         repeat winner, no-bid clause, conflict of interest)
  7. write run-log section into investigation page
  8. write candidate findings (raw/ for hard data, draft/ for narratives)
  9. log to <wiki>/log.md
```

## Wiki page templates

### Contractor page (`<wiki>/Contractors/<slug>.md`)

```yaml
---
title: <Legal Name>
type: contractor
created: YYYY-MM-DD
updated: YYYY-MM-DD
sources: [Vault/pdfs/...pdf]
investigations: [parks-contractors]
---

# <Legal Name>

## Overview
[2-3 sentences: who they are, what they do, where based]

## Principals & related entities
- [[person-name]] — role
- [[org-name]] — relationship

## City contracts (cumulative)
| Date | Department | Project | Amount | Source |
|---|---|---|---|---|
| 2024-03 | Parks | Riverwalk maintenance | $1.2M | [[Vault/pdfs/2024-03-15-abc123-riverwalk-award]] |

## Patterns observed
[agent-written, but every claim cites a vault entry]

## Open questions
[agent-written; operator can edit]
```

### Investigation run-log entry

```markdown
### Run 2026-04-26 14:00
- Pages crawled: 47 (12 new since last run)
- New entities: 3 contractors, 1 person, 2 RFPs
- Updated entities: 8 (added 4 new transactions to ACME Construction)
- Vaulted: 6 PDFs, 11 HTML pages
- Candidate findings:
  - draft/2026-04-26-acme-board-overlap.md (narrative — needs review)
  - raw/2026-04-26-q1-parks-awards.md (data drop — auto-publish)
- Blockers: parks-budget-2025.pdf returned 404 (was indexed last run; flagged Cartographer)
- Next-run hint: depth-crawl /vendors/acme-construction once a week (this contractor recurs)
```

## Candidate-finding rules

A finding is candidate **narrative** (→ `Findings/draft/`) when it asserts a *connection* between 2+ entities that wasn't in the source data verbatim. Examples:

- "Vendor X's principal is also on the board of [[NGO Y]] which received Parks funding"
- "Councilor Z voted on a contract awarded to a relative"
- "Vendor W has won 80% of no-bid Parks contracts since 2020"

A finding is **raw data** (→ `Findings/raw/`) when it's a fact directly from a source page:

- "Parks awarded $X to Y on date Z" — already in the source
- "RFP-2026-014 closed with 3 bidders"
- "Council vote 5-2 on item 14 (link)"

The agent **must NOT publish** narrative findings. They land in draft and wait for the human Reviewer.

## Pitfalls

- **Goal drift.** A weekly investigation run will accumulate everything tangentially related. Each run's analysis pass must re-anchor on the goal field — don't just dump every entity found.
- **Entity name normalization.** "ACME Construction LLC" / "ACME Construction" / "Acme Construction Co." → must reconcile through `civic-data-reporter`. Don't create three pages.
- **Crawl explosion.** Without depth + max_pages caps, a single seed can pull in the entire site. Hard caps required.
- **Stale source on re-run.** A URL that was good last run may be broken now. Don't fail the run; mark the source `status: broken` in the data, log it, continue.
- **JS-rendered pages.** Same Playwright fallback as sitemap-builder.
- **PDF parsing varies wildly.** Bid sheets vs. budget books vs. RFPs need different parsers. Parser registry, not one giant prompt.
- **Don't write narrative claims to entity pages.** Entity pages list facts. Connections live in `Findings/draft/`. This separation is the editorial firewall.
- **Cron overlap.** Two simultaneous runs on the same investigation must not race. Use a lock file in `<wiki>/Investigations/.locks/<slug>.lock`.

## Dependencies

- `sitemap-builder` (register mode)
- `civic-archivist` (every external resource)
- `civic-data-reporter` (entity + transaction upserts)
- Parser registry (one module per portal vendor)
- `obsidian` skill (for wikilink hygiene; entity name → existing-page lookup)

## Verification (acceptance criteria)

- ✅ Investigation file with 2 seeds and depth=2 produces a run-log entry within 30 minutes
- ✅ Entity pages created have at least 2 outbound wikilinks
- ✅ Every transaction recorded has a vault path source
- ✅ Re-run on unchanged inputs produces a "no changes" run-log entry, doesn't duplicate
- ✅ Re-run after new awards page detects new transactions and adds to existing contractor page
- ✅ Narrative finding lands in `Findings/draft/`, not `published/`
- ✅ Crashed mid-run leaves no half-written wiki pages (atomic writes)

## Open questions (for the operator)

1. Should depth crawl follow off-domain links (e.g., contractor's own website) or stay strictly on `.gov`? Default proposal: stay on `.gov`. Off-domain enrichment is a separate investigation type.
2. How aggressive should the LLM connection-finding pass be? Too aggressive → noisy draft folder. Too conservative → operator misses connections. Suggest tuning per-investigation via a `finding_threshold: low | medium | high` field.
3. Should re-runs preserve the prior run's draft findings, or sweep them into `archive/` if not promoted to published within N days?
