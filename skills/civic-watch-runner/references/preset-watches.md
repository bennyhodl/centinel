# Preset watches

Three presets ship with the skill, dropped at `<wiki>/Watches/_presets/` (read-only). The operator copies a preset to `<wiki>/Watches/<id>.yaml` to activate, then tunes the YAML in place. Never edit `_presets/` from the agent — those are upstream references.

---

## 1. `errant-spending.yaml`

**Type:** data (with optional narrative escalation)
**Default `auto_publish`:** `true` for the raw-data triggers (no-bid threshold, cost overrun); the LLM-narrative escalation is always draft.
**Default severity:** `high`
**Sitemap types targeted:** `contracts`, `rfps`, `budget`
**Expected hit rate:** 0–5 per week against tampa.gov data once tuned. First runs may flood — start with high thresholds.

```yaml
id: errant-spending
title: Errant spending detector
type: data
match:
  type: [contracts, rfps, budget]
  change: [added, changed]
  url_pattern: "/(procurement|budget|contracts)/.*"
rule: |
  -- runs against tampa.db (Data Reporter)
  SELECT t.id AS txn_id,
         t.txn_date,
         t.amount_usd,
         t.contract_method,
         e_to.canonical_name AS contractor,
         e_dept.canonical_name AS department,
         t.contract_ref,
         t.source_url
  FROM transactions t
  JOIN entities e_to ON e_to.id = t.to_id
  JOIN entities e_dept ON e_dept.id = t.from_id
  WHERE t.txn_date >= :since
    AND (
      -- no-bid above threshold
      (t.contract_method = 'no-bid' AND t.amount_usd > 50000)
      -- repeat winner: same contractor took >50% of last 10 dept awards
      OR EXISTS (
        SELECT 1 FROM (
          SELECT to_id, COUNT(*) AS n
          FROM transactions
          WHERE from_id = t.from_id AND txn_type = 'award'
          ORDER BY txn_date DESC LIMIT 10
        ) recent WHERE recent.to_id = t.to_id AND recent.n > 5
      )
      -- amendment grew base >25%
      OR (t.txn_type = 'amendment' AND t.amount_usd > 0.25 * (
        SELECT base.amount_usd FROM transactions base
        WHERE base.contract_ref = t.contract_ref AND base.txn_type = 'award'
      ))
    )
severity: high
auto_publish: true
paused: false
max_hits_per_run: 25
notes: |
  Each hit fires raw because the underlying data point (a single transaction row)
  is hard, citable, and dollar-and-date concrete. The LLM escalation pass
  (read the source vault entry, judge whether a justification narrative exists)
  is run by the Editor on the raw findings — not by this watch.

  Tune the no-bid threshold ($50k v0.1) once we see real volume. Tampa's purchasing
  threshold for council approval is typically $50k–$100k depending on category.
```

**Why three triggers, not one:** each captures a different failure mode of procurement integrity. No-bid is the cleanest signal. Repeat-winner exposes capture. Amendment-creep is the cheap-trick maneuver — bid low, amend high.

---

## 2. `corruption-signals.yaml`

**Type:** narrative (always — connections, never single-data-point claims)
**Default `auto_publish`:** **`false`, and the runner enforces narrative→draft regardless of this flag.** Setting `auto_publish: true` here is invalid and will be logged as a config error.
**Default severity:** `high`
**Sitemap types targeted:** `contracts`, `boards`, `ethics`, `personnel`
**Expected hit rate:** very low (0–2 per month). High-stakes; tune toward precision over recall.

```yaml
id: corruption-signals
title: Conflict-of-interest signals
type: narrative
match:
  type: [contracts, boards, ethics, personnel]
  change: [added, changed]
rule: |
  Flag this page if it indicates ANY of the following patterns:

  1. A board member, councilor, or city official has a financial interest
     (declared business, principal role, ownership) in an entity that is
     the recipient of a city contract, grant, or award visible on this page.
  2. A donor (per public donor records) received a city award within 12
     months of a documented donation to the awarding official's campaign.
  3. A registered lobbyist's client received a contract or favorable vote
     while the lobbyist's registration was active.
  4. A conflict-of-interest disclosure on this page mentions an entity that
     ALSO appears on this page as a contract recipient.
  5. Contractor principals share a residential address with a city official
     (low-confidence flag — surface for review, do not assert relationship).

  Do NOT fire on:
  - Generic mentions of an official without a financial linkage.
  - Coincidental name matches (common surnames, common business names).
  - Speculation: if you can't quote the conflict from the page, don't fire.
severity: high
auto_publish: false      # ENFORCED: narrative ALWAYS draft regardless
paused: false
max_hits_per_run: 10
notes: |
  This watch produces ONLY draft findings. The connection — not the data point —
  is the claim, and connections require human (Reviewer + Counsel) review.
  The runner enforces this even if auto_publish is mistakenly set to true.

  False positives are tolerable here; false negatives that erode public trust
  are worse. But because every hit goes to draft, the operator absorbs the
  noise — keep the rule TIGHT.
```

**Why never auto-publish:** corruption claims are reputational nukes. The data point alone (e.g. "X's company received a contract") is benign — the *connection* (X is on the awarding board) is the claim. Connections need human judgment. No exceptions, no flags, no overrides.

---

## 3. `policy-drift.yaml`

**Type:** narrative (LLM-only)
**Default `auto_publish`:** `false` (and enforced narrative→draft).
**Default `paused`:** **`true`** — ships disabled. Operator opts in.
**Default severity:** `medium`
**Sitemap types targeted:** `budget`, `boards`, `general` (policy / planning pages)
**Expected hit rate:** unpredictable; this watch is the most subjective.

```yaml
id: policy-drift
title: Policy drift detector
type: narrative
match:
  type: [budget, boards, general]
  change: [added, changed]
  content_kind: [document, listing]
rule: |
  Flag this page if it contains language consistent with ANY of the following
  policy patterns:

  1. Mandatory programs that override merit/process selection (e.g. equity
     quotas in procurement, set-asides that supersede competitive bidding).
  2. Redistribution mechanisms (a fee or tax that funnels into a fund with
     discretionary award authority — i.e. fee → fund → grantee).
  3. Central-planning language in zoning, housing, or business policy
     ("the city will direct", "preferred outcomes", "favored uses").
  4. Vague, unmeasurable goals attached to spending authority ("equity",
     "resilience", "stakeholder engagement") without metrics or sunset clauses.

  REQUIRE a verbatim quote from the page that demonstrates the pattern.
  No quote → no finding.
severity: medium
auto_publish: false
paused: true             # opt-in
max_hits_per_run: 5
notes: |
  This watch is politically loaded by design. Ships disabled. The operator
  enables it knowingly and absorbs the false-positive cost.

  The Reviewer should be involved on every hit — these are EDITORIAL claims,
  not factual ones. Keep the bar high: vague language in budget docs is
  endemic; the watch should fire only on PROCEDURAL drift (overriding merit,
  bypassing competition), not on word choice.
```

**Why opt-in:** the whole point of a watch system is operator-defined. This one ships off because its trigger surface is the most rhetorical, the hardest to tune, and the most likely to generate finding-fatigue.

---

## Sitemap-type → watch fit

Quick reference for which watches care about which sitemap entry types:

| Sitemap `type`  | errant-spending | corruption-signals | policy-drift |
|---|---|---|---|
| contracts        | ✅ | ✅ | — |
| rfps             | ✅ | — | — |
| budget           | ✅ | — | ✅ |
| boards           | — | ✅ | ✅ |
| ethics           | — | ✅ | — |
| personnel        | — | ✅ | — |
| general          | — | — | ✅ |
| meetings         | — | (rare) | (rare) |
| permits / press / project / profile / form / calendar / document | — | — | — |

If a watch needs to scan a type not in this table, add it via the `match.type` list — the runner doesn't gatekeep types, this is just the recommendation.
