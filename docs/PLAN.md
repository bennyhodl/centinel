---
title: Centinel — Civic Intelligence Agent
status: 🛠️ v0.1 in progress
created: 2026-04-25
updated: 2026-05-21
checkpoint: v0.1 — Phases 0–4 of pi-agent migration complete. Next: real tool implementations (qmd_search, db_query, vault_put, web_fetch are still stubs).
owner: Ben
---

# Centinel — Civic Intelligence Agent

> A self-hosted template (centinel-server, built on pi-coding-agent) that crawls a city's `.gov` to build a **described, navigable sitemap** of the entire civic data surface. The sitemap is the entrypoint. From there, the human launches investigations — "track contractors in parks," "find funding flows to NGOs," "follow a single councilor" — and agents fan out, populate a wiki, and surface findings against user-defined and preset watch criteria. Distributed as **fork-your-own**. Centinel is the reference deployment.

## Status

🧠 **Thinking.** Tampa meeting ingest already populates `~/wiki/Tampa/` (27 pages). Latest design reframe (2026-04-25): **sitemap is the central artifact and human entrypoint**; investigations are launched against it; meetings demote to one input among many.

## Thesis

Civic data is sprawling, unstructured, and *different shapes for different investigators*. A budget hawk wants funding flows. A reporter wants project timelines. An activist wants social policy. A candidate wants vote records. **No single ingestion strategy serves all of them — but a *described sitemap* of the entire `.gov` surface does.** Once the agent has crawled and labeled every path with "what's here," the human picks which threads to pull, and the agent fans out from there into wiki entities, contractors, funding ledgers, and findings.

The sitemap is **the dashboard**. Investigations are **the tasks**. Findings are **the output**. Everything is sourced, public-by-default, with editorial gates only on synthesized narratives.

## Why this approach

- **Sitemap-first, not crawl-first.** Earlier framing was "crawl everything." That ingests too much noise. Better: crawl-and-describe → human picks investigative paths → deep-crawl on those.
- **Sitemap is dual-purpose.** It's the human's browsing entrypoint *and* the agent's launching pad for LLM-driven investigations.
- **Agent does the work; human reviews and uses.** The agent ingests, structures, and presents. The human reads, picks threads, and writes their own stories with the raw material the agent surfaces. Centinel is a tool, not a journalist.
- **Document vault** — every source document (PDF, HTML page, transcript) is saved to disk. The city's website may take pages down; the vault is the durable record.
- **Wiki + Obsidian for storage.** Markdown, graph view, version-controllable, human-editable. `llm-wiki` skill produces it.
- **Fork-your-own template.** Each operator runs their own centinel-server. No central infra, no SaaS, no auth.
- **Existing Tampa meeting parser folds in** as one connector among many.
- **Findings are the deliverable.** Raw data publishes freely; synthesized stories ("this contractor connects to that donor") gate behind editorial review.

## Architecture vision (sitemap-centric)

```
                 ┌─────────────────────────────────────────────────┐
                 │  Bootstrap: operator forks repo, points at .gov │
                 └─────────────────────┬───────────────────────────┘
                                       │
                       ┌───────────────▼───────────────┐
                       │  Sitemap Builder Agent        │  ⭐ central artifact
                       │  - sitemap.xml + crawl        │
                       │  - per-URL classification     │
                       │  - per-URL description        │
                       │    ("what's here, what type") │
                       │  - emits: sitemap.md (wiki)   │
                       │  - + sitemap.json (machine)   │
                       └───────────────┬───────────────┘
                                       │
                       ┌───────────────▼───────────────┐
                       │  Sitemap (the human entry)    │
                       │  - browsable in Obsidian      │
                       │  - browsable in web app       │
                       │  - searchable (qmd)           │
                       │  - kept fresh by lint cron    │
                       └───────────────┬───────────────┘
                                       │
   ┌───────────────────────────────────┼───────────────────────────────────┐
   │                                   │                                   │
   │  Human launches investigations    │                                   │
   │                                   │                                   │
   ▼                                   ▼                                   ▼
┌──────────────┐               ┌──────────────┐               ┌──────────────┐
│ Investigation│               │ Investigation│               │ Watch Job    │
│ "track parks │               │ "follow      │               │ "anything    │
│  contractors"│               │  Councilor X"│               │  matching    │
│              │               │              │               │  presets"    │
│ depth-crawl  │               │ depth-crawl  │               │              │
│ + extract    │               │ + extract    │               │ continuous   │
└──────┬───────┘               └──────┬───────┘               └──────┬───────┘
       │                              │                              │
       └──────────────────────────────┼──────────────────────────────┘
                                      │
                       ┌──────────────▼──────────────┐
                       │  Wiki (Obsidian vault)      │
                       │  - Sitemap/                 │  (the map itself)
                       │  - Investigations/          │  (per-investigation pages)
                       │  - Projects/                │
                       │  - Contractors/             │
                       │  - People/                  │
                       │  - Orgs/                    │
                       │  - Funding/                 │
                       │  - RFPs/                    │
                       │  - Boards/                  │
                       │  - Meetings/                │
                       │  - Findings/                │
                       │     ├── raw/      (auto)    │
                       │     ├── draft/    (review)  │
                       │     └── published/          │
                       └──────────────┬──────────────┘
                                      │
   ┌──────────────────────────────────┼──────────────────────────────────┐
   │                                  │                                  │
┌──▼─────────┐                ┌───────▼────────┐                ┌────────▼──────┐
│  Web App   │                │ Chat Interface │                │ Briefings &   │
│  - sitemap │                │  - "what's     │                │ Findings      │
│    explorer│                │    in /parks?" │                │  - weekly     │
│  - search  │                │  - "show me    │                │  - watch hits │
│  - timeline│                │    contractor  │                │  - finding    │
│  - graph   │                │    flows"      │                │    posts      │
└────────────┘                └────────────────┘                └───────────────┘
```

## Document Vault

Every source document the agent encounters is saved to local disk. The website is **not** trusted to remain available — pages get taken down, links rot, PDFs get replaced. The vault is the durable record of what actually existed at the time of ingest.

```
<wiki>/Vault/
├── pdfs/
│   ├── <yyyy-mm-dd>-<sha8>-<slug>.pdf       # original PDF, never modified
│   └── <yyyy-mm-dd>-<sha8>-<slug>.md        # parsed markdown alongside
├── html/
│   └── <yyyy-mm-dd>-<sha8>-<slug>.html      # raw HTML capture
├── transcripts/
│   └── <yyyy-mm-dd>-<meeting-id>.txt        # raw meeting transcripts
├── images/
│   └── <yyyy-mm-dd>-<sha8>-<slug>.png       # captured screenshots, scanned docs
└── manifest.jsonl                            # one line per vaulted doc:
                                              # {url, sha256, fetched_at, type, path, sitemap_entry}
```

**Rules:**
- Vault entries are **immutable**. If a page changes, fetch a new vault entry; never overwrite.
- Every wiki claim links to vault path(s) for its source(s).
- Every sitemap entry's `content_hash` points at a specific vault entry.
- Vault paths are hash-prefixed so identical content dedupes.
- Web app serves vault files at stable URLs (`/vault/<path>`) so external readers can verify claims.
- No versioning at the wiki layer — the **vault is the version history**, augmented by git on the wiki itself.

## The Sitemap (central concept)

**Sitemap = labeled directory of the city's web surface.** One entry per discovered URL.

### Sitemap entry schema (proposed)

```yaml
- url: https://www.tampa.gov/procurement/awards
  type: contracts            # one of: meetings, contracts, rfps, budget, boards,
                             # permits, ethics, press, personnel, project, document, general
  description: |
    Page lists awarded city contracts grouped by department, updated quarterly.
    Each row links to a contract PDF.
  content_kind: index        # one of: index, document, listing, form, profile, news
  contains:
    - awarded contracts (table)
    - PDF links to award docs
  linked_entities: []        # populated as investigations run
  last_crawled: 2026-04-25
  content_hash: sha256:...
  parser: contracts-portal-tampa  # registry key
  crawl_freq: weekly
  status: active             # active | broken | excluded | needs_review
  notes: []                  # human notes, e.g. "added 2026-03-12 after RFP page reorg"
```

The sitemap lives at:
- `<wiki>/Sitemap/index.md` — human-readable, grouped by section, browsable in Obsidian
- `<wiki>/Sitemap/sitemap.json` — machine-readable, used by the agent and the web app

The web app renders the sitemap as a navigable tree with descriptions, search, and "investigate from here" buttons.

### Sitemap lint (cron)

A scheduled lint job keeps the sitemap fresh:
- Re-discover URLs (sitemap.xml + recursive crawl)
- Mark new URLs `needs_review` for the operator to classify/exclude
- Mark missing URLs `broken`
- Re-describe URLs whose `content_hash` changed
- Update `last_crawled`
- Append diff summary to `<wiki>/Sitemap/log.md`
- Output: lint report posted to operator (Discord/Telegram)

This replaces the earlier "change detection events" idea — the sitemap *is* the change-detection layer. Diffs surface as sitemap updates.

## Investigations

An **investigation** is a user-defined goal scoped to a starting point on the sitemap.

```yaml
# wiki/Investigations/parks-contractors.md (frontmatter)
---
title: Parks contractors over the last 5 years
goal: |
  Identify every contractor that has received parks-department funding,
  cumulative $$ awarded, and any cross-department repeat awards.
seeds:
  - https://www.tampa.gov/parks
  - https://www.tampa.gov/procurement/awards
status: active           # active | paused | done | archived
depth: 3                 # how many hops the agent follows
schedule: weekly         # how often the investigation re-runs
created: 2026-04-25
updated: 2026-04-25
---
```

The agent reads the investigation file, depth-crawls from the seeds, extracts entities, populates wiki pages, and writes a results section into the investigation page. Re-runs append updates with timestamps. The investigation page becomes the operator's working notebook for that thread.

### Investigation types

- **Topic dig** — "all funding flowing to NAACP-affiliated orgs"
- **Person follow** — "everything Councilor X touches: votes, attendance, sponsorships"
- **Money trace** — "every dollar from impact fees to where it lands"
- **Project timeline** — "the WOW project from first mention to today"
- **Contractor profile** — "everything about Vendor Y across departments and time"

These are templates the operator can clone and customize. centinel-server cron jobs run them on the configured schedule.

## Watch Jobs (the findings engine)

Distinct from investigations. **Watch jobs are continuous, preset-driven, sitemap-wide.** They scan everything new (sitemap diffs + new wiki pages) for matches against watch criteria.

### Built-in watch presets (configurable per operator)

1. **Communist / collectivist policy signals** — language patterns around redistribution, mandatory programs, central planning, equity-driven targets that override merit/process
2. **Errant spending** — no-bid awards, cost overruns >X%, repeat winners, late deliverables, contracts to entities without prior performance, line items that grew >Y% YoY without justification
3. **Corruption signals** — board member's company receives contract, donor → award correlation, lobbyist registration matches contract awardee, undisclosed conflicts in financial filings, family-name overlaps between officials and vendors

### User-extensible

Operators add their own watch criteria as YAML files in `<wiki>/Watches/`. Each watch is a prompt + scope + schedule + severity threshold. Hits → `<wiki>/Findings/raw/` for the operator to triage.

### Watch job lifecycle

```
sitemap diff or new wiki page
        │
        ▼
  watch matchers run (LLM + heuristic)
        │
        ├─→ no match → silent
        │
        └─→ match → Finding/raw/<date>-<slug>.md
                         │
                         ├─→ if "raw data": auto-publish to web
                         │
                         └─→ if "narrative/connection": → draft/ → human review → published/
```

## Findings & editorial gate

Two-track publishing:

### Track 1: Auto-publish (raw data)
- New entities (contractors, projects, RFPs, board members)
- Hard data points: contract awarded, RFP closed, vote recorded, budget line filed
- Sitemap changes
- Investigation results pages

These ship to the public web app immediately, sourced.

### Track 2: Editorial review (narrative findings)
- Connection findings — "this contractor's principal is a major donor to this councilor"
- Pattern findings — "vendor X has won 80% of parks contracts under no-bid clauses"
- Synthesized stories — anything that connects 2+ entities into a claim

These land in `<wiki>/Findings/draft/`. Operator (or designated editor) reviews, refines, decides:
- **Publish** → move to `<wiki>/Findings/published/`, surfaces on web app
- **Pursue** → keep as draft, agent runs follow-up depth-crawl to gather more evidence
- **File for record** → move to `<wiki>/Findings/archive/`, not surfaced publicly

This is the journalistic core: agent surfaces *candidate stories*; human decides what becomes a published claim.

## People & entity rules

- **Officials** (council, mayor, staff, board appointees) — entity pages, surfaced on web
- **Contractor principals** — entity pages, surfaced on web
- **Civic leaders** (philanthropic rich, NGO heads, repeat power-broker speakers) — entity pages, surfaced on web. The principle is **tag power, not citizens**.
- **Private citizens** (one-off speakers, unaffiliated mentions) — no entity page; their words are referenced in source pages only
- **Threshold** — 3+ distinct mentions OR official role OR identified leadership position in a tracked org
- **Archival** — when a person no longer holds a tracked role and hasn't been mentioned in 12+ months, page moves to `<wiki>/_archive/People/`. Sitemap lint flags candidates.

## Operator UX — visual, not push

The operator interacts with the system primarily by *visiting* it, not by being notified. Surfaces:

- **Web app sitemap explorer** — landing page. New URLs since last visit are highlighted; descriptions are visible inline; "investigate from here" buttons launch new investigations.
- **Findings feed** — visual list, freshest first, raw vs. reviewed clearly tagged.
- **Investigations index** — all live investigations with last-run timestamp and "what's new" badge.
- **Obsidian** — the curator's deeper view. Open the wiki, browse the graph, follow wikilinks.

No Discord/Telegram alerts in v0.1. The whole product is "go look when you want to look." Briefings (weekly digest) are the only push surface, and that's a published artifact, not an alert.

## Agent stack

All work is agent-run on centinel-server cron. Three new skills + reuse of existing.

| Component | Type | Skill | Trigger |
|---|---|---|---|
| Sitemap Builder | new skill | `sitemap-builder` | manual + monthly cron |
| Sitemap Lint | reuses Sitemap Builder | `sitemap-builder` (lint mode) | weekly cron |
| Investigator | new skill | `civic-investigator` | per-investigation cron |
| Watch Runner | new skill | `civic-watch-runner` | nightly cron over sitemap diffs + new wiki pages |
| Vault Fetcher | utility (no skill) | called by all above | inline |
| Briefings | reuse | `humanized-writing` | weekly cron |
| Wiki maintenance | reuse | `llm-wiki` lint | weekly cron |
| Web app + chat | Next.js app | n/a | always-on |

Each new skill is documented in `~/plans/centinel/research/skills/<name>.md` (TBD — next planning step).



### 1. Sitemap Builder Agent ⭐
- Crawl `.gov` (sitemap.xml + recursive)
- Per-URL: classify type, generate description, identify content kind, suggest parser
- Emit `<wiki>/Sitemap/index.md` + `sitemap.json`
- Operator approves at category level; per-URL exclusion list for noise

### 2. Sitemap Lint (cron)
- Re-crawl, diff, update, flag, log
- Posts summary report to operator

### 3. Investigation Engine
- YAML-frontmatter investigation pages in `<wiki>/Investigations/`
- Depth-crawl from seeds, extract entities, populate wiki
- Re-runs on schedule, appends updates
- Templates for common investigation types

### 4. Watch Engine (findings)
- Preset watches (communist/spending/corruption) + user-defined
- Matchers run on sitemap diffs and new wiki pages
- Hits → `Findings/raw/` with auto-publish vs. review classification

### 5. Parser registry
- One module per clerk vendor / portal vendor (Granicus, Legistar, CivicPlus, OpenGov, Bonfire, etc.)
- Registry keyed by parser name, referenced from sitemap entries
- Existing Tampa meeting parser is the first registry entry

### 6. Wiki (Obsidian vault)
- `~/wiki/<City>/` per operator
- Layout: Sitemap, Investigations, Projects, Contractors, People, Orgs, Funding, RFPs, Boards, Meetings, Findings, Watches
- All cross-linked; Obsidian graph view exposes the network

### 7. Web app
- Public read-only, deploys anywhere
- Sitemap explorer (the dashboard)
- Search (qmd lex+vec)
- Entity pages with timeline + relationships
- Findings feed (auto-published + reviewed)
- Investigation result pages
- Subscribe (RSS / email per entity or per investigation)

### 8. Chat interface
- Discord/Telegram bots + web chat
- Backed by wiki + qmd
- Cited answers with stable URLs
- Personas later

### 9. Briefings
- Weekly digest (cron-drafted, human-published)
- Watch hit alerts (push)

## Technical considerations

### Stack
- centinel-server (TypeScript, built on `@mariozechner/pi-coding-agent`) — owns roles, cron, qmd, and the `delegate` tool
- Markdown wiki, git-versioned
- Parser registry (extensible modules)
- Next.js web app
- Obsidian as the curator's view

### Constraints & principles
- Sitemap is source of truth for "what exists at the city"; wiki is source of truth for "what we know about it"
- Every public claim traces to raw source
- Tag power, not citizens
- Investigator-led, not opinionated
- No CMS, no central infra, no auth gate

## Open questions (current)

1. **Existing repo access** — still pending, blocks audit
2. **Sitemap entry schema** — does the schema above hold up under a real Tampa crawl? Validates with the recon spike
3. **Investigation schema** — same question; design will only firm up after running one end-to-end
4. **Watch preset rubrics** — "communist policy signals" needs a concrete prompt + heuristic. Same for spending and corruption. Each preset needs its own page + iteration.
5. **Parser vendor #2** — confirm Tampa's clerk vendor; pick one other for the registry test
6. **Crawl politeness** — bot UA string, rate limits, robots.txt policy
7. **Editorial workflow UI** — wiki PR-style is simplest; web app review queue is nicer. Defer until we have findings to review.
8. **Federation** — out of scope for v1, but schema must not preclude
9. **Person archival policy** — 12 months without mention seems right; needs a real test
10. **Watch criteria UI** — operators write YAML by hand for v1? Web form later?

## Resolved

- ~~Hosting~~ → fork-your-own centinel-server template, public good
- ~~Brand~~ → `centinel` template, operators fork to `<city>-doge`
- ~~Funding~~ → public good, no billing
- ~~People rule~~ → tag power, not citizens; 3+ mentions or official/leadership role
- ~~Crawl strategy~~ → sitemap-first, investigations on demand
- ~~Editorial gate~~ → split: raw data auto, narratives reviewed
- ~~Findings approach~~ → preset watches + user-defined, both feeding `Findings/raw/`
- ~~Sitemap descriptions~~ → LLM-generated, human reviews via web/Obsidian; descriptions are themselves an investigation entrypoint
- ~~Investigations~~ → always-alive, re-runnable, never "complete"; analyses spawn from them as separate artifacts
- ~~Operator role~~ → reads & uses; agent does ingest/structure/present; human writes their own stories
- ~~Source persistence~~ → document vault on disk; every PDF/page/transcript saved immutably; vault is version history; no separate versioning needed
- ~~Notification model~~ → visual on web app, no push alerts in v0.1
- ~~Collaboration~~ → operators all interact with the same agent; multi-user is a same-instance concern, not federation

## MVP proposal

**Goal:** Tampa, sitemap-first end-to-end loop.

**In scope (v0.1):**
- Sitemap Builder Agent + sitemap lint cron
- 1-2 investigation templates (topic-dig + contractor-profile)
- Watch engine with one preset wired up (errant spending — most concrete)
- Parser registry with Tampa meetings + Tampa procurement
- Wiki structure as defined above
- Web app: sitemap explorer + entity renderer + findings feed + qmd search
- Discord chat bot
- Weekly digest cron

**Out of scope (v0.1):**
- Second city
- Contractor SunBiz/donor fan-out research
- All three watch presets fully tuned
- Setup wizard polish (Tampa is hand-configured)
- Findings editorial UI (use wiki PR flow for v0.1)
- Graph visualization
- Federation

**Done when:**
- `<wiki>/Sitemap/index.md` exists and accurately describes tampa.gov's surface
- One investigation runs end-to-end and produces a populated investigation page
- Watch engine fires on at least one real sitemap change
- Web app shows sitemap, lets visitors search, lists findings, links to entity pages
- Chat bot answers a real question with citations
- Weekly digest publishes Mondays via cron

## Next steps

1. Repo audit (blocked on access)
2. **Sitemap Builder spike** against tampa.gov — produces a draft sitemap.md and sitemap.json. Validates the schema.
3. **Watch preset writing** — draft the prompts/rubrics for communist / errant spending / corruption presets. Each gets its own page in `<wiki>/Watches/_presets/`.
4. **Investigation template writing** — draft 2-3 templates (topic-dig, contractor-profile, person-follow).
5. Web app v0
6. Chat bot v0
7. Promote to 🔬 Investigating after sitemap spike
