---
title: Centinel — Editor Answer Sources
status: 🔒 Locked v1
created: 2026-04-28
parent: EDITOR_PERSONA.md
---

# Editor Answer Sources

When a human asks the Editor a question in `/chat`, the Editor's answer is
sourced from the wiki/DB material — never from its own training, never
hallucinated, never from the sitemap alone.

This file codifies the source priority. The Editor's system prompt enforces it.

## The priority list

Answer from these sources, in order:

1. **`<wiki>/_data/<city>.db`** (Datasette/SQLite)
   The structured truth. Entities, contractors, projects, transactions,
   funding flows, the methodology log. Every analytical claim ("ACME has won
   $4.2M in parks contracts since 2021") must resolve here or be marked as
   uncited.

2. **`<wiki>/Vault/`**
   The evidence base. PDFs, transcripts, HTML captures, images — each with a
   sidecar summary. Every quoted fact, dollar amount, or date claim links to
   a vault path that the human can click through and verify.

3. **`<wiki>/Findings/`** (raw + published)
   Already-synthesized claims with their own citations. Reuse rather than
   re-derive.

4. **`<wiki>/Investigations/<slug>.md`**
   Active and archived investigation pages with accumulated results sections.
   The investigation file itself is the canonical record of what's known.

5. **`<wiki>/Entities/`** wiki pages
   Biographies, relationship graphs, narrative pages on each tracked person/
   org/contractor/project. Reads as journalism; cites back to vault and DB.

6. **QMD search across the wiki** — **always available, always tried**
   See "Why QMD is mandatory" below.

## Why QMD is mandatory

QMD is the local hybrid (BM25 + vector + reranker) search engine over the
wiki. The Editor and every specialist agent **must** use QMD when answering
freeform questions, because:

- The DB only knows what was extracted into structured rows.
- The vault sidecars hold paragraphs of context the DB doesn't capture.
- Findings and entity pages are written-up narrative — searchable by phrase
  and by meaning, not by SQL.
- Wiki cross-references (`[[Entities/contractor/acme]]`) are how investigators
  encode relationships before they're structured.

A question like "what's the latest weirdness at the parks department?" will
return zero rows from a SQL query — but QMD finds the three findings, two
investigation pages, and one transcript paragraph that mention it.

**Rule: every Editor and specialist agent that answers questions calls QMD as
part of its retrieval step, not as a fallback.** Skip-QMD answers are
forbidden in the system prompt.

```
# In every agent's system prompt:
Before answering any freeform question, you MUST call qmd-search at least
once. Do not answer "I don't know" or "I have no information" without first
running QMD against the question.
```

## What the sitemap is *not*

The sitemap (`<wiki>/Sitemap/index.md` + `sitemap.json`) is **not** an answer
source. It's a **map for crawling and investigation**. The Editor uses it to:

- Decide where to seed an investigation (which sub-tree to crawl).
- Resolve a specific URL the operator referenced ("show me /procurement/contracts").
- Audit coverage (what hasn't been crawled yet, what's stale).
- Answer literal navigation questions ("where on tampa.gov is the budget?").

It does **not** appear in answer-citations for knowledge questions. If the
Editor finds itself wanting to cite the sitemap, it's a signal that the
underlying material hasn't been ingested into vault/DB yet — which means the
right answer is "I don't have that yet; queueing the Investigator to ingest
[URL]."

## Answer flow

```
human asks question
  │
  ├─ Editor calls qmd-search(question) → top-N hits
  │   (always, no exceptions)
  │
  ├─ Editor queries DB for any structured hooks
  │   (entity names, dollar thresholds, date ranges)
  │
  ├─ Editor reads relevant Findings, Investigations, Entities pages
  │   (using qmd hits to find them)
  │
  ├─ Editor reads vault sidecars for any cited PDFs/transcripts
  │
  ├─ Has enough material to answer with citations?
  │   ├─ YES → answer with citations, every claim → vault path or DB query
  │   ├─ ALMOST → delegate_task(skill=civic-investigator) for sync analysis
  │   └─ NO → write inbox/investigator/<task>.md, tell human "queued"
  │
  └─ Editor answers in chat. No citation = no claim.
```

## Hard rules

- **Every claim cites.** Vault path, DB methodology query ID, or wiki page
  reference. No source → "I don't have a source for that yet."
- **QMD always runs.** No exception. Even if the DB has the answer, QMD runs
  too — narrative context often matters.
- **Sitemap is for navigation only.** Never cited for knowledge claims.
- **Never invent.** "I don't know" is always a valid answer; making something
  up is not.
- **Specialists follow the same rules.** Investigator, Archivist, Data
  Reporter, Watch Runner — when they answer questions (sync via
  `delegate_task` or async via outbox) they use the same source priority and
  the same QMD-mandatory rule.

## Acceptance criteria

- ✅ Editor system prompt (`EDITOR_PERSONA.md`) enforces QMD-first retrieval.
- ✅ All specialist skill SKILL.md files include the same QMD-mandatory rule.
- ✅ Editor's tool use logs show a `qmd-search` call on every chat-question turn.
- ✅ No answer in chat ever cites the sitemap as the source for a factual
  knowledge claim.
- ✅ When the Editor queues an inbox task, the chat response explicitly says
  "queued [task] to [role], expect results [when]."
