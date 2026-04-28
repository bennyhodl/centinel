# Public Datasette views

The web app's `/db` route serves Datasette over a *sanitized* read surface, not the raw `tampa.db`. This document specifies what's filtered and why, plus sample `CREATE VIEW` statements. The actual SQL lives at `<wiki>/_data/public-views.sql` and is loaded by `scripts/init_public_views.sh`.

## Implementation choice

Two options were considered:

1. **Separate `tampa-public.db`** rebuilt nightly via `INSERT INTO ... SELECT FROM` from views.
2. **Views on the live DB**, served by Datasette in readonly mode pointed at `tampa.db` with a query whitelist.

**We use option 2** — views on the live DB. Reasons:

- Datasette runs readonly via `--immutable` flag or by opening with `mode=ro`; no risk of write.
- Views update instantly; no nightly rebuild lag (the operator wants `M-<id>` lookups to work the moment the methodology row is inserted).
- One DB file to back up, not two.

The wrapper `scripts/init_public_views.sh` reads `<wiki>/_data/public-views.sql` and applies it to `tampa.db`. Because `CREATE VIEW IF NOT EXISTS` is idempotent, it can run on every schema migration.

## Filter rules

Views applied across the board:

1. **Confidence floor.** Every view filters `WHERE confidence >= 0.7`. Anything lower is unconfirmed and not for public consumption.
2. **No raw entity hints.** The Archivist sidecar `entity_hints` are NOT in the DB; they live in vault sidecars. The only way a name reaches the public DB is by being upserted as an `entities` row by Data Reporter.
3. **Status filter.** `entities.status = 'active'` only; merged and archived rows are operator-internal.
4. **Confidential investigation transactions.** Transactions whose `source_vault_path` starts with `Vault/_confidential/` (operator-flagged) are excluded.
5. **PII scrub on `metadata_json`.** Persons' DOB and home-address fields are stripped before exposure.
6. **Methodology is fully public** — that's the point. All columns of `methodology` are visible.
7. **Sources** — `vault_path` and `kind` only; `sha256` and `first_referenced` are not exposed (sha256 is fine to expose later but adds no public value in v0.1).

## Sample views

```sql
-- public-views.sql
-- Loaded into tampa.db by scripts/init_public_views.sh.

CREATE VIEW IF NOT EXISTS v_entities_public AS
SELECT
  id, type, canonical_name, slug, wiki_path,
  first_seen, last_seen,
  -- strip PII fields from JSON (whitelist projection):
  json_object(
    'role',        json_extract(metadata_json, '$.role'),
    'jurisdiction',json_extract(metadata_json, '$.jurisdiction'),
    'ein_present', json_extract(metadata_json, '$.ein') IS NOT NULL
  ) AS metadata,
  confidence
FROM entities
WHERE status = 'active'
  AND confidence >= 0.7;

CREATE VIEW IF NOT EXISTS v_transactions_public AS
SELECT
  t.id, t.txn_date, t.from_id, t.to_id, t.amount_usd,
  t.txn_type, t.contract_ref, t.rfp_id, t.is_no_bid, t.description,
  t.source_vault_path, t.confidence
FROM transactions t
WHERE t.confidence >= 0.7
  AND t.source_vault_path NOT LIKE 'Vault/_confidential/%';

CREATE VIEW IF NOT EXISTS v_relationships_public AS
SELECT id, from_id, to_id, rel_type, start_date, end_date,
       source_vault_path, confidence, notes
FROM relationships
WHERE confidence >= 0.7;

CREATE VIEW IF NOT EXISTS v_events_public AS
SELECT id, event_date, event_type, entity_id, related_ids,
       payload_json, source_vault_path, confidence
FROM events
WHERE confidence >= 0.7;

CREATE VIEW IF NOT EXISTS v_aliases_public AS
SELECT a.id, a.entity_id, a.alias, a.confidence
FROM aliases a
JOIN entities e ON e.id = a.entity_id
WHERE e.status = 'active'
  AND a.confidence >= 0.7;

-- methodology fully public
CREATE VIEW IF NOT EXISTS v_methodology_public AS
SELECT id, run_date, query_label, sql, row_count, result_hash, asked_by, used_in, notes
FROM methodology;

CREATE VIEW IF NOT EXISTS v_sources_public AS
SELECT vault_path, kind FROM sources;
```

Datasette's `--config default_facet_size:10 --metadata <metadata.json>` controls which views it surfaces. The metadata JSON whitelists exactly the `v_*_public` views; the underlying tables are not exposed even though they exist in the same DB.

## Verification

- `sqlite3 tampa.db "SELECT name FROM sqlite_master WHERE type='view';"` shows all `v_*_public`.
- `curl localhost:8001/db.json` enumerates only public views.
- A row with `confidence = 0.5` exists in `entities` but does NOT appear in `v_entities_public`.
- A row with `source_vault_path = 'Vault/_confidential/foo.pdf'` exists in `transactions` but does NOT appear in `v_transactions_public`.
