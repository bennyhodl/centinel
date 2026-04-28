---
title: civic-data-reporter (skill spec)
status: 🧠 Specced
created: 2026-04-26
agent_role: Data Reporter
parent: ../README.md
---

# `civic-data-reporter` — Skill Spec

## Purpose

Owns the entity database. Imports records, normalizes names, dedupes, runs the daily summary statistic, answers the operator's queries on demand, documents methodology, backs up. The relational spine that the wiki, the investigations, and the watch runner all sit on top of. Maps directly to the Spotlight Data Reporter — files FOIAs (no, the human does), runs queries (yes), is a reporter not a "data monkey" — every analysis is reproducible.

## When this skill activates

- Called inline by `civic-investigator` for `upsert_entity` / `upsert_transaction`
- Called inline by `civic-archivist` for `record_entity_hint` (tentative entities pending reconciliation)
- Operator runs an ad-hoc query (chat command or web app)
- Daily cron: cleaning routines (dedup, normalize)
- Weekly cron: backup, methodology audit, "data state" memo
- Watch runner queries it for criteria like "no-bid awards" or "repeat winners"

## Storage

**SQLite** as primary store, **Datasette** as the read-only web view (per the Spotlight stack's recommendation). Database file at `<wiki>/_data/tampa.db`. SQL schema versioned via Alembic-style migrations in `<repo>/db/migrations/`. Datasette serves at `:8001` for the web app to embed.

## Schema (initial)

```sql
-- Entities: anything we track. One row per canonical entity.
CREATE TABLE entities (
  id            INTEGER PRIMARY KEY,
  type          TEXT NOT NULL,          -- person | org | contractor | dept | board | rfp | project
  canonical_name TEXT NOT NULL,
  slug          TEXT NOT NULL UNIQUE,    -- matches wiki page slug
  wiki_path     TEXT,                    -- e.g. Contractors/acme-construction.md
  first_seen    DATE NOT NULL,
  last_seen     DATE NOT NULL,
  status        TEXT NOT NULL DEFAULT 'active',  -- active | archived | merged
  merged_into_id INTEGER REFERENCES entities(id),
  metadata_json TEXT                     -- type-specific fields (DOB for persons, EIN for orgs, etc.)
);

-- Aliases: variant spellings, dba names, prior names, all map to one entity.
CREATE TABLE aliases (
  id          INTEGER PRIMARY KEY,
  entity_id   INTEGER NOT NULL REFERENCES entities(id),
  alias       TEXT NOT NULL,
  source_vault_path TEXT,
  confidence  REAL NOT NULL,             -- 0.0–1.0
  UNIQUE(entity_id, alias)
);

-- Relationships: typed edges between entities.
CREATE TABLE relationships (
  id          INTEGER PRIMARY KEY,
  from_id     INTEGER NOT NULL REFERENCES entities(id),
  to_id       INTEGER NOT NULL REFERENCES entities(id),
  rel_type    TEXT NOT NULL,             -- principal_of | board_member_of | spouse_of | donor_to | employed_by | parent_of_org
  start_date  DATE,
  end_date    DATE,
  source_vault_path TEXT NOT NULL,
  confidence  REAL NOT NULL,
  notes       TEXT
);

-- Transactions: dollars moving between entities at a date.
CREATE TABLE transactions (
  id            INTEGER PRIMARY KEY,
  txn_date      DATE NOT NULL,
  from_id       INTEGER REFERENCES entities(id),     -- payer (e.g., Parks Department)
  to_id         INTEGER REFERENCES entities(id),     -- payee (contractor)
  amount_usd    REAL NOT NULL,
  txn_type      TEXT NOT NULL,           -- award | payment | reimbursement | grant | donation | fee
  contract_ref  TEXT,                    -- contract / RFP / award number
  rfp_id        INTEGER REFERENCES entities(id),
  is_no_bid     BOOLEAN,
  description   TEXT,
  source_vault_path TEXT NOT NULL,
  confidence    REAL NOT NULL
);

-- Events: discrete actions (votes, meetings attended, sponsorships).
CREATE TABLE events (
  id          INTEGER PRIMARY KEY,
  event_date  DATE NOT NULL,
  event_type  TEXT NOT NULL,             -- vote | meeting | sponsorship | appointment | filing | hearing
  entity_id   INTEGER REFERENCES entities(id),    -- principal subject
  related_ids TEXT,                       -- JSON array of entity ids
  payload_json TEXT,                      -- vote outcome, meeting topic, etc.
  source_vault_path TEXT NOT NULL,
  confidence  REAL NOT NULL
);

-- Methodology log: every analytical query the team relied on, reproducibly recorded.
CREATE TABLE methodology (
  id          INTEGER PRIMARY KEY,
  run_date    DATE NOT NULL,
  query_label TEXT NOT NULL,
  sql         TEXT NOT NULL,
  result_summary TEXT,
  used_in     TEXT,                       -- finding slug, briefing slug, etc.
  notes       TEXT
);
```

Add columns and tables as new investigation types demand. Migrations versioned.

## Operations exposed

```python
# Inline upserts (Investigator + Archivist call these)
upsert_entity(type, canonical_name, hints=[], metadata={}, source_vault_path) -> entity_id
upsert_alias(entity_id, alias, confidence, source_vault_path)
upsert_relationship(from_id, to_id, rel_type, start_date, end_date, source_vault_path, confidence)
upsert_transaction(txn_date, from_id, to_id, amount_usd, txn_type, source_vault_path, ...) -> txn_id
upsert_event(event_date, event_type, entity_id, payload, source_vault_path, ...) -> event_id

# Operator queries (chat / web)
run_query(label, sql, save_to_methodology=True) -> rows
common_queries.repeat_winners(department, since)
common_queries.no_bid_awards(since)
common_queries.donor_to_award_overlap(councilor_id)
common_queries.contractor_cumulative(contractor_id)

# Maintenance
reconcile_entity_hints(min_confidence=0.85)   # promotes hints to canonical entities
normalize_names()                              # nightly
detect_duplicates() -> [(id_a, id_b, score)]
merge_entities(keeper_id, dupe_id)             # operator-confirmed only
backup() -> path
data_state_memo() -> markdown
```

## Name normalization (the unglamorous 60%)

This is the part that makes or breaks the database. Algorithm sketch:

1. **Pre-normalize.** Lowercase, strip punctuation, expand common abbreviations (`LLC|Inc|Corp|Co.`), strip suffixes for comparison (keep canonical with suffix).
2. **Fuzzy match candidates.** Use trigram similarity (Postgres-style, implemented over SQLite via custom function or RapidFuzz in app code) for top-N candidate matches above threshold 0.80.
3. **LLM disambiguation pass for borderline cases (0.80–0.95).** Prompt with both names + context (other transactions, addresses, dates) → "same entity / different / unsure". Below 0.80: separate entities. Above 0.95: auto-merge with alias record.
4. **Operator review queue.** All "unsure" decisions land in `<wiki>/_data/_review/<date>-entity-merges.md` for human approval before merge. Never silently merge an unsure case.
5. **Address-aware** for orgs/contractors: same name + same address = strong same-entity signal.
6. **Name + role-aware** for persons: "John Smith (Parks director)" vs "John Smith (Council District 3)" likely different.

## Daily summary statistic (the "fresh visualization")

Every morning the cron picks one of a rotating set:
- count of new entities since yesterday by type
- top-10 contractors by cumulative spend (last 90 days)
- network graph of co-occurring names from the last week's transactions
- count of pending investigations × pages crawled per investigation

Renders to `<wiki>/_data/_daily/<YYYY-MM-DD>.md` with an embedded chart (Datasette URL or static SVG).

## Methodology document (continuous)

Every query that produces a published claim must be in `methodology` table AND in `<wiki>/_data/methodology.md`. Format per query:

```markdown
### Q-2026-04-26-001: Parks contractors cumulative since 2021
**Used in:** [[Findings/published/2026-04-30-parks-no-bid-pattern]]
**SQL:**
\`\`\`sql
SELECT e.canonical_name, SUM(t.amount_usd) AS total
FROM transactions t
JOIN entities e ON e.id = t.to_id
WHERE t.txn_date >= '2021-01-01'
  AND t.from_id IN (SELECT id FROM entities WHERE canonical_name='Tampa Parks Department')
GROUP BY e.canonical_name
ORDER BY total DESC;
\`\`\`
**Result summary:** 47 contractors, top 5 account for 68% of spend.
**Caveats:** Excludes payments after 2026-Q1 not yet ingested; assumes Parks Department alias map current as of 2026-04-25.
```

The standards/copy editor (human) reads this when reviewing a finding, the same way a Spotlight standards editor reads the back-up file.

## Backup (weekly cron)

1. `sqlite3 tampa.db .backup tampa-YYYY-MM-DD.db`
2. Encrypt with age (operator's pubkey).
3. Push to two destinations (operator-configured: e.g., a local NAS + an offsite S3-like).
4. Verify by decrypting the latest backup and running `PRAGMA integrity_check`.
5. Append result to `<wiki>/_data/_backups.md`.

## Pitfalls

- **Schema drift will hurt.** Resist adding ad-hoc columns. Either it's a first-class concept (new column with migration) or it goes in `metadata_json`. Don't sprinkle string fields named `extra1`, `extra2`.
- **Confidence scores are mandatory.** Every row carries one. Findings that lean on low-confidence rows must surface that fact in the methodology entry.
- **Auto-merge is dangerous.** "John Smith" → never auto-merge. Always require operator approval for person merges. Org/contractor merges with strong signals (same name + same address + same EIN) can auto-merge.
- **Time-bounded relationships.** A board member from 2018–2020 is NOT a board member in 2024. Always carry start_date / end_date and respect them in queries.
- **Source linkage is non-negotiable.** Every row has a `source_vault_path`. A row without one is unverifiable and gets purged in nightly cleanup.
- **Datasette exposure.** The Datasette read-only view is fine for the operator's own machine. Public web app must NOT expose the raw DB — it queries via a curated API. Otherwise we leak draft entity reconciliations and low-confidence rows.
- **Migration discipline.** Every schema change ships a migration. Never alter the schema by hand in prod.
- **Race on inline upserts.** Investigator + Archivist may upsert the same entity simultaneously. Use SQLite `INSERT ... ON CONFLICT` with the slug unique constraint.

## Dependencies

- `sqlite3` (stdlib)
- `rapidfuzz` (name similarity)
- `datasette` + `datasette-render-markdown` plugin
- `age` for backup encryption
- LLM call for borderline disambiguation
- `pandas` for ad-hoc queries the operator runs from a notebook

## Verification (acceptance criteria)

- ✅ Inserting "ACME Construction LLC" then "Acme Construction" produces 1 entity, 2 aliases
- ✅ Inserting "John Smith" then "John Smith" with no disambiguating context lands both in `_review/` queue, no merge
- ✅ Common-query `repeat_winners('Parks', '2021-01-01')` returns the same result for the same DB snapshot — reproducible
- ✅ Every `transactions` row has a non-null `source_vault_path`
- ✅ Backup decrypts cleanly and passes `PRAGMA integrity_check`
- ✅ Datasette web view shows masked confidence < 0.5 rows ("low-confidence — see review queue")
- ✅ Methodology table grows by one row per published finding

## Open questions (for the operator)

1. Postgres vs SQLite long-term? Default proposal: SQLite + Datasette is enough through v1; revisit at 1M+ rows.
2. Graph DB for relationships (Neo4j Community per Spotlight stack)? Default proposal: defer; SQL + recursive CTE handles 1–2 hops fine; revisit if web app needs interactive network exploration.
3. Should the methodology document be public on the web app (transparency) or operator-internal? Default proposal: public for any methodology row referenced by a published finding; private otherwise.
