# Finding classification: raw vs draft

Every hit lands in one of two folders:

- `<wiki>/Findings/raw/` — auto-published. The web app surfaces these immediately on next build.
- `<wiki>/Findings/draft/` — gated for human review. The Editor or Reviewer promotes, kills, or pursues.

This file is the rubric. Memorize it.

---

## The decision tree

```
For each hit:
  if watch.type == "narrative":
      → DRAFT  (always; no exceptions; auto_publish flag ignored)

  elif watch.type == "data":
      if watch.auto_publish == true
         AND hit has a hard data point (single concrete fact)
         AND hit has at least one citation (source_url or vault_path)
         AND citation confidence >= 0.8:
              → RAW
      else:
              → DRAFT

  else:
      → DRAFT  (unknown type, fail safe)
```

**The narrative-→-draft rule is hard.** Even if a narrative watch is mistakenly configured with `auto_publish: true`, the runner overrides it and emits to draft. Log the misconfig as a tuning request.

---

## What is a "hard data point"?

A single concrete fact, expressible in one sentence, anchored by:

- A specific dollar amount, OR
- A specific date + party + action, OR
- A specific document with a vault path.

Examples that ARE hard data points:

- "ACME Construction received a $1.2M no-bid contract on 2026-04-15." (amount + date + party + action)
- "Contract amendment CCN-2026-031 grew the base award from $400k to $1.1M." (specific document, specific delta)
- "RFP-2026-008 closed with a single bidder." (specific RFP, specific count)

Examples that are NOT hard data points (→ draft, even if the watch is data-typed):

- "ACME has won several recent contracts." (no specific count, no specific dates)
- "Spending in parks-department appears elevated." (no anchor)
- "The procurement page shows several entries." (no claim)

If the data row backing a hit is missing the anchors above, the hit is not raw-eligible. Route to draft.

---

## What counts as a citation?

In order of preference:

1. **Vault path** — `Vault/pdfs/2026-04-15-acme-riverwalk.pdf`. Owned by the Archivist; immutable. Strongest.
2. **Source URL** — the `.gov` URL the data was extracted from. Acceptable if the URL is in `<wiki>/Sitemap/sitemap.json` with `status: active`.
3. **Database row reference** — `tampa.db transactions.id=12345`. Only valid if the underlying transaction has its own source citation in `events` or `transactions.source_url`.

A finding with no citation in any of these forms is **discarded**. Log the discard, do not write the file.

---

## Confidence scoring

For data watches, confidence comes from:

- Citation type (vault path = 1.0, sitemap URL = 0.9, db-only = 0.7).
- Field completeness (all key columns non-null = 1.0; partial = 0.6).

For narrative watches, confidence is the LLM's self-reported confidence, scaled by the citation's confidence.

A raw finding requires `confidence >= 0.8`. Below that → draft.

---

## Worked examples

### Example 1: clear raw

**Watch:** `errant-spending` (data, `auto_publish: true`)
**Hit:** SQL row with `txn_id=8821, amount_usd=750000, contract_method='no-bid', txn_date='2026-04-22', contractor='ACME', source_url='https://www.tampa.gov/procurement/awards/2026-031'` and the URL is in the sitemap with `status: active`.

→ **RAW.** Hard data point (amount + date + party). Citation present. Confidence ~0.9.

### Example 2: clear draft (narrative)

**Watch:** `corruption-signals` (narrative)
**Hit:** LLM returned `match: true, quote: "Board member John Smith disclosed ownership of ACME Holdings.", confidence: 0.85`. The page also lists ACME Holdings as a contract recipient.

→ **DRAFT.** Narrative watches are always draft. The connection (board member ↔ contract recipient) is the claim, and connections need human review.

### Example 3: data watch, but missing citation

**Watch:** `errant-spending` (data, `auto_publish: true`)
**Hit:** SQL row but `source_url` is null and there's no vault path. Just a DB ID.

→ **DRAFT** — and add an `## Open Questions` block: "Source citation missing — Archivist may not have vaulted this yet." On next run, if the Archivist has filled in the path, the finding can be promoted (manually, by the operator).

### Example 4: data watch, partial data

**Watch:** `errant-spending` (data, `auto_publish: true`)
**Hit:** SQL row with `amount_usd=null, contract_method='no-bid'`. No dollar anchor.

→ **DRAFT.** Missing the amount means it's not a hard data point — it's a flag for review.

### Example 5: misconfigured narrative watch with `auto_publish: true`

**Watch:** `policy-drift` (narrative) with `auto_publish: true` (operator error).

→ **DRAFT** (runner overrides). Additionally: emit a tuning request to Editor noting the misconfig. Don't silently honor.

### Example 6: low-confidence LLM hit

**Watch:** `corruption-signals`. LLM returns `match: true, quote: "...", confidence: 0.45`.

→ **Discarded** (below 0.6 narrative threshold). Logged but no file written.

### Example 7: dedup across runs

**Watch:** `errant-spending`. Same `(watch_id, url, content_hash)` triple already produced a finding two runs ago.

→ **Discarded by `dedup_hits.py`** before classification. The finding from two runs ago is the canonical artifact.

---

## Escalation rules

If a draft finding has been sitting unreviewed for >14 days AND severity is `high`, the runner adds a `nudge` line to the next run-summary outbox message:

> "draft `2026-04-15-corruption-signals-abc.md` has been awaiting review for 14 days; severity=high"

Don't auto-promote drafts. Don't auto-archive them. The Editor / Reviewer / operator owns disposition.

If `<wiki>/Findings/draft/` accumulates >50 unreviewed files, the runner emits a higher-priority "draft backlog" note. The fix is always operator review — never silent cleanup.

---

## What this rubric does NOT cover

- **Promoting a draft to published.** That's `civic-investigator` / Editor / human territory. The runner only writes drafts.
- **Killing a finding.** Operator moves it to `Findings/archive/` with `status: killed`.
- **Cross-finding dedup beyond `hit_hash`.** If two watches independently surface "ACME no-bid contract", both fire. The Editor consolidates. The runner only de-dups within a single watch via `hit_hash`.
