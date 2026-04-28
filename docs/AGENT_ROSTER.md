---
title: Tampa-DOGE — Agent Roster (LOCKED)
status: 🔒 Locked v1
created: 2026-04-26
parent: README.md
---

# Tampa-DOGE Agent Roster

Mapping the Spotlight investigative model (`ORG_STRUCTURE_AND_WORKFLOW.md`) onto Tampa-DOGE's runtime agents. Locked 2026-04-26.

## Principles

1. **Human is top-level.** The operator IS the Investigations Editor + Executive Editor + Deputy Managing Editor collapsed into one role. The human decides what to investigate, what to publish, and what gets killed.
2. **Agents do the legwork.** Crawling, OCR, entity extraction, database maintenance, watch matching, draft synthesis — all agent. No agent has authority to publish a narrative finding.
3. **Source protection is human territory.** SecureDrop, Signal, Tails, Faraday pouches, in-person meets, FOIA drafting/filing, source ledger — humans only. Out of scope for the agent stack.
4. **No agent talks to the outside.** Agents only ingest from public surfaces and write to local wiki/vault. All outbound communication (calls, emails, FOIA letters, right-of-reply contacts) is human.

---

## The roster

| # | Spotlight role | Tampa-DOGE name | Who | Skill / Component |
|---|---|---|---|---|
| 1 | Executive Editor | **Operator-in-Chief** | 🧑 Human | — |
| 2 | Deputy Managing Editor | *(collapsed into Operator)* | 🧑 Human | — |
| 3 | Investigations Editor | **Operator** (you) | 🧑 Human | — directs all agents |
| 4 | — *(new role)* | **Cartographer** | 🤖 Agent | `sitemap-builder` |
| 5 | Lead Reporter | **Investigator** | 🤖 Agent | `civic-investigator` |
| 6 | Reporter (2nd seat) | **Archivist** | 🤖 Agent | `civic-archivist` |
| 7 | Data Reporter | **Data Reporter** | 🤖 Agent | `civic-data-reporter` |
| 8 | News Researcher | **Watch Runner** | 🤖 Agent | `civic-watch-runner` |
| 9 | Copy/Standards Editor | **Reviewer** | 🧑 Human | findings draft → published |
| 10 | In-House Counsel | **Counsel** | 🧑 Human | legal review pre-publish |
| 11 | Visual Journalist | *(folded into web app)* | 🤖 Agent | web app renderers |
| 12 | Web Producer | **Web Producer** | 🤖 Agent (build) + 🧑 Human (ops) | Next.js app |
| 13 | — | **Briefings Writer** | 🤖 Agent | `humanized-writing` |
| 14 | — | **Librarian** | 🤖 Agent | `llm-wiki` (lint mode) |
| 15 | SecureDrop maintainer | *(out of scope — human territory)* | 🧑 Human | — |

**Five new skills:** `sitemap-builder`, `civic-investigator`, `civic-archivist`, `civic-data-reporter`, `civic-watch-runner`. Two reused: `humanized-writing`, `llm-wiki`.

---

## Agent responsibilities (one paragraph each)

### 🤖 Cartographer — `sitemap-builder`
Owns the sitemap. Crawls `.gov`, classifies every URL by type and content kind, generates the LLM description, suggests a parser, emits `<wiki>/Sitemap/index.md` + `sitemap.json`. Lint mode runs weekly: re-discovers URLs, marks `needs_review` / `broken`, re-describes changed pages, appends diff to `Sitemap/log.md`. The Cartographer has no Spotlight analog because the Spotlight model assumes the beat is already known — Tampa-DOGE has to recon the beat first.

### 🤖 Investigator — `civic-investigator`
Runs investigations end-to-end. Reads a YAML investigation file, depth-crawls from seeds, extracts entities into wiki pages, populates contractors / projects / funding ledgers, writes a results section into the investigation page. Re-runs on schedule and appends. Drops candidate connection findings into `Findings/draft/` for human review. Files everything to the vault before claiming it.

### 🤖 Archivist — `civic-archivist`
Document intake queue. Every PDF, HTML capture, transcript, image: hash → vault → OCR → index → tag → 1-3 paragraph summary alongside. Cross-checks names/dates/dollars from documents against the Data Reporter's database; flags discrepancies. Maintains vault manifest integrity. The unglamorous backbone — corresponds to the 2nd-seat Reporter who reads 50–200 pages a day.

### 🤖 Data Reporter — `civic-data-reporter`
Owns the entity database (SQLite + Datasette interface, per the Spotlight stack). Imports new records, normalizes names ("J. Smith" / "John A. Smith" → one entity), dedups, builds the daily summary statistic, runs operator queries on demand ("show me every transaction over $50K between these two entities 2018–2021"). Documents methodology. Backs up weekly. Pairs with the Archivist when new documents land.

### 🤖 Watch Runner — `civic-watch-runner`
Runs continuously over sitemap diffs and new wiki pages. Matches against preset watches (errant spending, corruption signals, communist/collectivist policy signals) plus user-defined YAML watches in `<wiki>/Watches/`. Hits go to `Findings/raw/`. Auto-classifies hit as "raw data" (auto-publish) or "narrative connection" (gates to draft for human review). Maps to the News Researcher role: pulls public records continuously, builds background files on tracked persons.

### 🤖 Briefings Writer — `humanized-writing`
Weekly digest. Reads the week's findings + sitemap deltas + investigation updates, drafts a published artifact in operator's voice. Human reviews before publishing.

### 🤖 Librarian — `llm-wiki` (lint mode)
Wiki health: broken wikilinks, orphaned pages, stale entity pages (no mention >12 months → archive candidate), tag normalization. Weekly cron. Posts a lint report; takes no destructive action without operator approval.

### 🤖 Web Producer — Next.js app
Renders the sitemap explorer, entity pages, findings feed, investigation result pages, qmd search. Read-only public site. Build/deploy is automated; the SecureDrop side of the Spotlight Web Producer's job is **not** in scope — that's a human-operated separate machine.

---

## Human responsibilities (explicit)

The operator wears all the editorial-authority hats:

- **Direct investigations.** Pick what to investigate, write the YAML, set seeds and depth, schedule re-runs.
- **Set watches.** Tune presets, write user-defined watches, set severity thresholds.
- **Approve findings.** Track 2 (narrative findings in `Findings/draft/`) → publish, pursue, or file.
- **Right-of-reply.** Any contact with a named subject. Agents never call, email, or message anyone.
- **FOIA / public records requests.** Drafted, filed, tracked by the human; the agent only ingests responses once received.
- **Source protection.** SecureDrop, Signal, Tails, Faraday pouches, source ledger, codenames. Out of scope for the agent stack — human-operated separate infrastructure.
- **Counsel.** Legal review before publication of narrative findings.
- **Standards review.** Final pass on every published narrative — every name spelled right, every date confirmed, every quote matches the source.
- **Briefings publish.** Agent drafts, human signs off.

---

## What this roster does *not* include (deliberately)

- **No autonomous publisher.** No agent publishes a narrative finding without human review.
- **No source-comms agent.** Signal, SecureDrop, burner phones, in-person meets — all human.
- **No interview agent.** Agents do not call, email, or message external parties.
- **No FOIA-filing agent.** Agents track FOIA returns once they arrive (the Archivist ingests them) but do not draft or file.
- **No legal-review agent.** Counsel is human, full stop.
- **No federation layer.** Each operator runs their own instance.

---

## Locked. Next planning step

Spec the five new skills (`sitemap-builder`, `civic-investigator`, `civic-archivist`, `civic-data-reporter`, `civic-watch-runner`) under `~/plans/tampa-doge/research/skills/`. Deferred per operator direction — return to this when ready to build.
