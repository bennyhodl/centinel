---
title: civic-watch-runner (skill spec)
status: 🧠 Specced
created: 2026-04-26
agent_role: Watch Runner
parent: ../README.md
---

# `civic-watch-runner` — Skill Spec

## Purpose

Continuously scan sitemap diffs and new wiki content against preset and user-defined watch criteria. Drop hits to `<wiki>/Findings/raw/` (auto-publish) or `<wiki>/Findings/draft/` (narrative needs human review). Maps to the Spotlight News Researcher (continuous public-records monitoring) plus a domain-specific anomaly detector.

## When this skill activates

- Nightly cron after `sitemap-builder` lint completes
- After a `civic-investigator` run completes (scoped to that investigation's outputs)
- Operator manually triggers `run-watches [watch-name]`

## Inputs

```yaml
mode: nightly | post_investigation | manual
watch_dir: <wiki>/Watches            # user-defined YAML watches
preset_dir: <wiki>/Watches/_presets   # built-in presets
scope:
  since: 2026-04-25T00:00:00         # nightly: last successful run timestamp
  diff: <wiki>/Sitemap/log.md#latest  # last sitemap diff section
  new_pages: [list]                   # post-investigation: new wiki pages this run
  investigation: parks-contractors    # post-investigation only
config:
  max_findings_per_run: 50            # safety cap
  default_severity_threshold: medium  # low | medium | high
```

## Outputs

1. **Findings** in `<wiki>/Findings/raw/<YYYY-MM-DD>-<watch-slug>-<short>.md` (auto-publish) or `<wiki>/Findings/draft/...` (narrative)
2. **Watch run log** at `<wiki>/Watches/_runs/<YYYY-MM-DD>.md` — what watches ran, scope, hits per watch, runtime
3. **Append to `<wiki>/log.md`**: `## [YYYY-MM-DD] watch-run | N watches, M hits`

## Watch file schema

```yaml
# <wiki>/Watches/no-bid-awards.yaml
---
name: no-bid-awards
description: |
  Surface any contract award that's flagged no-bid OR has a single bidder
  AND amount > $50,000.
severity: high            # low | medium | high — determines whether finding gates to draft
scope:
  - sitemap_types: [contracts, rfps]
  - new_transactions_with: { is_no_bid: true }
  - new_transactions_with: { sole_source: true, amount_min: 50000 }
mode: heuristic           # heuristic | llm | hybrid
heuristic_query: |
  -- runs against civic-data-reporter db
  SELECT t.id, t.txn_date, e_to.canonical_name AS contractor,
         t.amount_usd, t.contract_ref
  FROM transactions t
  JOIN entities e_to ON e_to.id = t.to_id
  WHERE t.txn_date >= :since
    AND (t.is_no_bid = 1 OR (t.txn_type='award' AND t.amount_usd > 50000
         AND NOT EXISTS (SELECT 1 FROM events ev WHERE ev.event_type='rfp_bidder'
                          AND ev.entity_id != t.to_id
                          AND json_extract(ev.payload_json, '$.contract_ref') = t.contract_ref)))
finding_kind: raw         # raw (auto-publish) | narrative (draft)
finding_template: |
  # No-bid award: {contractor} — ${amount_usd:,.0f}
  - **Date:** {txn_date}
  - **Contract:** {contract_ref}
  - **Source:** [[{source_vault_path}]]
  - **Why surfaced:** {reason}
schedule: nightly
status: active
---
```

## Built-in watch presets

Three presets ship in `<wiki>/Watches/_presets/`. Each is a starter rubric the operator tunes:

### `errant-spending.yaml` (most concrete; ship first)

Heuristic + LLM hybrid. Triggers:
- No-bid award above threshold
- Cost overrun >X% on a tracked project
- Repeat winner (>50% of last 10 awards in a department)
- Contract amendment that grows a base contract by >Y%
- Award to entity with no prior performance history in any department
- YoY line-item growth >Z% without budget book justification narrative

LLM pass on suspicious items: read the source vault entry, check if there's a justification narrative; if yes and reasonable → don't fire; if absent/weak → fire.

### `corruption-signals.yaml`

Heuristic-driven. Triggers:
- A board member's company receives a contract from that board's domain
- A councilor votes on a contract awarded to an entity sharing an address with their declared business
- Donor (per public donor records the operator imports) receives award within N months of donation
- Contractor's principals share surnames or addresses with city officials (low-confidence flag for operator review)
- Lobbyist registration matches contract awardee
- Conflict-of-interest disclosure mentions an entity that later receives a contract

All hits gate as `narrative` — never auto-publish a corruption finding. Always to `Findings/draft/` for human Reviewer + Counsel.

### `policy-drift.yaml`

LLM-driven. Triggers on new wiki content matching language patterns associated with:
- Mandatory programs that override merit/process selection
- Equity-driven targets that supersede competitive bidding
- Redistribution mechanisms (fee→fund→grantee with discretionary award)
- Central-planning language in zoning/housing/business policy

This one is the most-LLM-judgment-heavy and the most subjective; ships disabled by default, operator opts in. Hits always gate as `narrative`.

## Algorithm

```
on_run(scope):
  1. load all active watches from preset_dir + watch_dir
  2. for each watch:
       a. determine input set per watch.scope:
          - sitemap diff entries since `since`
          - new wiki pages in `new_pages` (post-investigation mode)
          - new database rows (transactions/events) since `since`
       b. mode:
          - heuristic: run heuristic_query/rule against db; collect hits
          - llm: for each candidate item in scope, LLM pass with watch.description as criterion
          - hybrid: heuristic narrows; LLM filters
       c. for each hit:
          - render watch.finding_template with hit context
          - destination = raw/ if watch.finding_kind=='raw' else draft/
          - write Findings file with frontmatter:
              ---
              watch: no-bid-awards
              severity: high
              kind: raw
              source_vault_paths: [...]
              entities: [acme-construction, parks-department]
              created: <date>
              ---
          - log
  3. write watch run log
  4. summary line to operator notification
```

## Findings file rules

### `Findings/raw/` (auto-publish)

- One concrete fact per file
- Always cites at least one vault path
- Web app surfaces immediately on next build
- Frontmatter `kind: raw`, `auto_published: true`
- The agent doesn't editorialize — just states the fact and the source

### `Findings/draft/` (human review)

- Narrative claim connecting 2+ entities OR pattern across multiple data points
- Cites every supporting vault path AND every database query (methodology references)
- Frontmatter `kind: narrative`, `status: draft | reviewing | published | archived | killed`
- Reviewer (human) reads, edits, decides
- Promote: `mv draft/<file> published/<file>` + bump frontmatter status
- Kill: `mv draft/<file> archive/<file>` + frontmatter `status: killed`, `kill_reason: <text>`
- Pursue: keep in draft, add a `pursuit_notes:` field, optionally trigger a follow-up investigation

The draft folder is the agent ↔ human handoff. Without active operator review the draft folder fills up — which is correct behavior, not a bug. The Briefings Writer surfaces draft folder size in the weekly digest as a nudge.

## Severity → gating logic

| Severity | finding_kind override | gates to |
|---|---|---|
| `low` | as declared | `raw/` if declared raw, else `draft/` |
| `medium` | force `narrative` for any LLM-driven match | `draft/` |
| `high` | force `narrative` always | `draft/` |

Translation: high-stakes watches (corruption) never auto-publish, even if the underlying evidence is a single hard data point. The reasoning: the *connection*, not the data, is the claim.

## Pitfalls

- **False-positive flooding.** A poorly-tuned watch fires 50 hits a night and the operator stops looking. Each watch must declare a `max_hits_per_run`; on overflow the watch auto-pauses and posts a tuning request.
- **LLM hallucinated connections.** "This contractor is the spouse of the councilor" — based on what? LLM-only watches must surface their evidence and cite vault paths. Watch runner enforces: every LLM-produced hit MUST include source_vault_paths. No path → discard hit and log.
- **Stale heuristic_query.** The query references a column that's been migrated. Watch runner catches DB errors, marks the watch `status: broken`, posts to operator. Doesn't crash the whole run.
- **Watches running on stale data.** Always run watches AFTER sitemap-builder lint and AFTER civic-data-reporter daily clean. Cron ordering matters.
- **Drift between preset and operator's tuned version.** Operator copies preset to `Watches/no-bid-awards.yaml` and edits. Preset gets updated upstream. Skill must NOT overwrite operator-tuned watches; presets at `_presets/` are read-only references.
- **`policy-drift` is politically loaded.** This is by design — the operator chose to surface it. But watches must never *publish* their interpretation. Ship disabled-by-default, gate to draft, lean heavily on operator and Reviewer.
- **Don't watch retroactively unless asked.** When a new watch is added, default scope is "from now forward". A `--backfill` flag triggers retroactive scan only if the operator explicitly requests it.

## Dependencies

- `civic-data-reporter` (heuristic queries, source confidence)
- `civic-archivist` (vault path resolution for source citations)
- LLM calls for `mode: llm` and `mode: hybrid`
- `<wiki>/Sitemap/log.md` parser (for diff scope)
- Notification channel (briefings cron uses this; watch runner posts a one-line summary)

## Verification (acceptance criteria)

- ✅ A new no-bid award row in transactions triggers exactly one finding in `Findings/raw/`
- ✅ A corruption-signal hit ALWAYS lands in `Findings/draft/`, never `raw/`
- ✅ Disabling a watch (`status: paused`) skips it next run; re-enabling resumes
- ✅ A watch hitting >max_hits_per_run auto-pauses and emits a tuning request
- ✅ An LLM hit without vault paths is discarded and logged, not written
- ✅ Two simultaneous watch runs don't double-fire on the same input window (idempotency via run timestamp)
- ✅ Promoting a draft finding to `published/` removes it from "draft awaiting review" count

## Open questions (for the operator)

1. The three presets ship with rubric placeholders — they need real-Tampa tuning. Plan: ship `errant-spending` first against tampa.gov data, iterate on rubric for 2 weeks, then ship `corruption-signals`, then `policy-drift`.
2. Should the watch runner draft a brief headline + 2-sentence summary on each finding (LLM call) or just template-fill? Default proposal: template-fill for `raw/`; LLM-summarize for `draft/` so the Reviewer has a starting point.
3. How do we feed the watch runner external data (donor records, SunBiz registrations)? That's not in `.gov` — needs separate ingestion. Defer to v0.2.
