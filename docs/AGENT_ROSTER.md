---
title: Centinel — Agent Roster (LOCKED)
status: 🔒 Locked v1
created: 2026-04-26
parent: README.md
---

# Centinel Agent Roster

Mapping the Spotlight investigative model (`ORG_STRUCTURE_AND_WORKFLOW.md`) onto Centinel's runtime agents. Locked 2026-04-26. See `AGENT_INVOCATION.md` for how each profile is actually launched.

## Principles

1. **Human is top-level.** The operator IS the Investigations Editor + Executive Editor + Deputy Managing Editor collapsed into one role. The human decides what to investigate, what to publish, and what gets killed.
2. **Agents do the legwork.** Crawling, OCR, entity extraction, database maintenance, watch matching, draft synthesis — all agent. No agent has authority to publish a narrative finding.
3. **Source protection is human territory.** SecureDrop, Signal, Tails, Faraday pouches, in-person meets, FOIA drafting/filing, source ledger — humans only. Out of scope for the agent stack.
4. **No agent talks to the outside.** Agents only ingest from public surfaces and write to local wiki/vault. All outbound communication (calls, emails, FOIA letters, right-of-reply contacts) is human.

---

## Hermes profile mapping

Each agent is a **Hermes profile** under `~/.hermes/profiles/<name>/` with its own `config.yaml`, `skills/`, `memories/`, `sessions/`, and `cron/`. They communicate **only** through the wiki filesystem (`<wiki>/_runtime/inbox/<agent>/`, `outbox/<agent>/`, `status/<agent>.md`) per RUNTIME_PROTOCOL.md.

Invocation is via thin `bin/centinel-<role>` wrappers that exec `hermes --profile <role>`; cron jobs use `hermes --profile <role> cron create '<sched>' --skill <skill> --name <n> "<prompt>"` (note: `--profile` is the global flag, BEFORE the subcommand; `--skill` is singular, repeatable). See `AGENT_INVOCATION.md`.

| Profile | Skills loaded | Identity | Cron | Notes |
|---|---|---|---|---|
| **(default Hermes agent)** | `sitemap-builder` + Editor system prompt | **Editor + Cartographer** | weekly sitemap lint | Fronts the `/chat` API via OpenAI-compatible endpoint. Knows the city's website inside-out because it owns the sitemap directly. |
| `investigator` | `civic-investigator` | Investigator | per-investigation (registered by Editor when operator launches one) | |
| `archivist` | `civic-archivist` | Archivist | every 15min + inline calls | |
| `data-reporter` | `civic-data-reporter` | Data Reporter | every 6h + inline DB queries | |
| `watch-runner` | `civic-watch-runner` | Watch Runner | every 4h after sitemap diffs | |

**5 entities total** (1 main + 4 profiles). Memory is per-profile — agents have no shared brain; coordination happens via filesystem messages.

**Why Editor + Cartographer collapse:** the Editor's job is to *know* the city. The sitemap is its mental map. Splitting them would force inbox round-trips for every "what's at /procurement?" — which the Editor should answer instantly. The sitemap-builder skill loads into the main agent so the Editor can both query AND maintain the sitemap.

**Briefings Writer + Librarian** stay as separate cron jobs but reuse existing skills (`humanized-writing`, `llm-wiki`) and run inside the main Hermes agent — they don't need their own profiles.

---

## The roster (Spotlight mapping)

| # | Spotlight role | Centinel name | Who | Profile / Skill |
|---|---|---|---|---|
| 1 | Executive Editor | **Operator-in-Chief** | 🧑 Human | — |
| 2 | Deputy Managing Editor | *(collapsed into Operator)* | 🧑 Human | — |
| 3 | Investigations Editor | **Operator** (you) | 🧑 Human | — directs all agents |
| 4 | Investigations Editor (LLM) + recon | **Editor + Cartographer** | 🤖 Agent | default Hermes profile + `sitemap-builder` |
| 5 | Lead Reporter | **Investigator** | 🤖 Agent | `investigator` profile, `civic-investigator` |
| 6 | Reporter (2nd seat) | **Archivist** | 🤖 Agent | `archivist` profile, `civic-archivist` |
| 7 | Data Reporter | **Data Reporter** | 🤖 Agent | `data-reporter` profile, `civic-data-reporter` |
| 8 | News Researcher | **Watch Runner** | 🤖 Agent | `watch-runner` profile, `civic-watch-runner` |
| 9 | Copy/Standards Editor | **Reviewer** | 🧑 Human | findings draft → published |
| 10 | In-House Counsel | **Counsel** | 🧑 Human | legal review pre-publish |
| 11 | Visual Journalist | *(folded into web app)* | 🤖 Agent | web app renderers |
| 12 | Web Producer | **Web Producer** | 🤖 Agent (build) + 🧑 Human (ops) | Next.js app |
| 13 | — | **Briefings Writer** | 🤖 Agent | runs in default profile, reuses `humanized-writing` |
| 14 | — | **Librarian** | 🤖 Agent | runs in default profile, reuses `llm-wiki` (lint mode) |
| 15 | SecureDrop maintainer | *(out of scope — human territory)* | 🧑 Human | — |

**Five new skills:** `sitemap-builder` (loads into default profile), `civic-investigator`, `civic-archivist`, `civic-data-reporter`, `civic-watch-runner` (each loads into its own profile). Two reused: `humanized-writing`, `llm-wiki` (run in default profile).

---

## Agent responsibilities (one paragraph each)

### 🤖 Editor + Cartographer — main Hermes agent (default profile)
Owns the sitemap **and** the chat surface. Crawls `.gov` (sitemap-builder skill), classifies every URL by type and content kind, generates LLM descriptions, suggests parsers, emits `<wiki>/Sitemap/index.md` + `sitemap.json`. Lint mode runs weekly. The same agent fronts `/chat` via Hermes' OpenAI-compatible endpoint — the operator chats with the entity that *built* the sitemap, so it can answer "what's at /procurement?" instantly without an inbox round-trip. As Editor, it synthesizes findings, drafts narratives, registers investigations, and tunes watches per `EDITOR_PERSONA.md`. The Editor never publishes without operator approval.

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

Spec the five new skills (`sitemap-builder`, `civic-investigator`, `civic-archivist`, `civic-data-reporter`, `civic-watch-runner`) under `~/plans/centinel/research/skills/`. Deferred per operator direction — return to this when ready to build.
