# Match-criteria DSL

The `match:` block on a watch YAML filters the changed-entry list (from the latest sitemap diff) down to a candidate set. Each filter ANDs with the others. A watch's `rule:` is then evaluated against each candidate.

The DSL is intentionally narrow: it's a sieve, not a query language. Heavy joins live in the `rule:` (SQL for data watches, prose for narrative).

---

## Operators

### `type` — list of sitemap entry types
Matches if the entry's `type` is in the list.

```yaml
match:
  type: [contracts, rfps]
```

Allowed values come from `sitemap-builder`'s allowed types:
`meetings | contracts | rfps | budget | boards | permits | ethics | press | personnel | project | document | profile | calendar | form | general`

### `url_pattern` — regex (Python `re.search` semantics)
Matches if the entry's URL matches the pattern.

```yaml
match:
  url_pattern: "/procurement/(awards|amendments)/.*"
```

For glob-like simplicity, prefix with `glob:` to use fnmatch:

```yaml
match:
  url_pattern: "glob:*/budget/*.pdf"
```

### `change` — list of change kinds
Filters by what kind of diff produced this entry. Values: `added | changed | broken | any`.

```yaml
match:
  change: [added, changed]
```

`broken` is rarely useful (a 404 isn't normally a finding) — but a watch could surface "previously-public document is now broken" as a transparency signal.

### `content_kind` — list of sitemap content kinds
Same allowed values as `sitemap-builder`: `index | document | listing | form | profile | news | calendar | search`.

```yaml
match:
  content_kind: [document, listing]
```

### `value_threshold` — DB-joined numeric threshold (data watches only)
For watches whose candidates ultimately resolve to DB rows (transactions, events). The runner joins the changed-entry URL set with `tampa.db` and applies the threshold.

```yaml
match:
  value_threshold:
    field: transactions.amount_usd     # table.column
    op: ">"                            # > | >= | < | <= | = | !=
    value: 100000
```

Only one `value_threshold` per watch in v0.1. Compose multiple conditions in the `rule:` SQL instead.

### `new_only` — bool
Shorthand for `change: [added]`. Useful when a watch is intended to fire only on first-sighting.

```yaml
match:
  new_only: true
```

### `entity_in` — list of entity slugs (advanced; data watches only)
The entry must reference an entity in the list (resolved via `<wiki>/Entities/`). Useful for focused watches like "anything touching `mayor-acme`".

```yaml
match:
  entity_in: [acme-construction, parks-department]
```

---

## Combining filters

All filters in a `match:` block AND together. There is **no OR at the DSL level** — express OR in the `rule:` instead, or split into two watch YAMLs.

```yaml
match:
  type: [contracts]
  url_pattern: "/procurement/.*"
  change: [added]
  value_threshold:
    field: transactions.amount_usd
    op: ">"
    value: 50000
```

This reads: "newly-added contract pages under `/procurement/`, joined with a transaction whose amount > $50k."

---

## What's NOT supported in v0.1

- **Joins beyond DB-backed rules.** The DSL doesn't join two sitemap entries together. If you need that pattern, write a narrative watch and let the LLM read both pages, or write a data watch with SQL that does the join.
- **Full-text content search.** The DSL filters on metadata (URL, type, kind, change). To match on page text, use a narrative watch — the LLM call sees the content.
- **Time windows beyond `since` (last run).** Every watch implicitly runs on entries changed since `last_run`. There's no "last 30 days" filter — that's what the cron cadence is for.
- **OR at the DSL level.** Use multiple watches or push the OR into the rule.
- **Negative matches** (`type_not`, `url_pattern_not`). Not in v0.1; if you need exclusion, narrow the positive matchers.

---

## Rule evaluation (where the real logic lives)

After the DSL produces a candidate set, the watch's `rule:` is evaluated against each candidate.

### Data rules

The `rule:` is SQL run against `tampa.db`. The runner binds:

- `:since` — last successful run timestamp (ISO-8601).
- `:url` — the candidate entry's URL (when applicable).
- `:content_hash` — the candidate entry's content hash.

Each row of the result set is one hit. The runner expects every row to have:

- A column that contains a citation (a `source_url`, a `vault_path`, or a `contract_ref` resolvable via the Archivist).
- Enough context to render the finding template.

If the SQL fails (column missing, syntax error, etc.), the runner sets the watch's `status: broken` in its YAML frontmatter and posts a tuning request to the Editor — but does NOT crash the rest of the run.

### Narrative rules

The `rule:` is prose. The runner makes one LLM call per candidate:

```
You are evaluating whether a wiki page or sitemap entry triggers a watch criterion.

WATCH RULE:
{rule_text}

PAGE URL: {url}
PAGE CONTENT (excerpt, ~2000 chars):
{content}

Output strict JSON:
{
  "match": true|false,
  "reason": "<one sentence why>",
  "quote": "<verbatim quote from the page that supports the match; empty string if match=false>",
  "confidence": <0.0-1.0>
}

Rules:
- If match=true and quote is empty, that's an error — set match=false.
- Quote MUST appear verbatim in the page content.
- Be conservative. False positives erode operator trust.
```

The runner verifies:

1. JSON parses.
2. If `match: true`, `quote` is non-empty AND appears verbatim in `content` (substring check). If not, discard the hit and log `llm_no_citation`.
3. `confidence` is in `[0, 1]`.

A confidence threshold of `0.6` is the v0.1 default for narrative hits to even reach the draft folder. Tune per watch via a `confidence_min: 0.7` field on the YAML if needed.
