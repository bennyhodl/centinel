# Tampa-DOGE DB schema (canonical)

Location: `<wiki>/_data/tampa.db`. SQLite, WAL mode. The Data Reporter is the sole writer.

`PRAGMA user_version` tracks schema version. Each migration adds one to it. The current version on a fresh `init_db.py` install is **1**.

## Tables

### `entities`

One row per canonical entity. The slug matches the wiki page slug under `<wiki>/Entities/<type>/<slug>.md`.

```sql
CREATE TABLE entities (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  type            TEXT NOT NULL CHECK(type IN ('person','org','contractor','dept','board','rfp','project')),
  canonical_name  TEXT NOT NULL,                          -- normalized form (see name-normalization.md)
  slug            TEXT NOT NULL UNIQUE,                   -- kebab-case, matches wiki path
  wiki_path       TEXT,                                   -- e.g. Entities/contractors/acme-construction.md
  first_seen      TEXT NOT NULL,                          -- ISO-8601 date
  last_seen       TEXT NOT NULL,
  status          TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active','archived','merged')),
  merged_into_id  INTEGER REFERENCES entities(id),
  metadata_json   TEXT,                                   -- type-specific JSON blob
  confidence      REAL NOT NULL DEFAULT 0.5
                    CHECK(confidence BETWEEN 0.0 AND 1.0),
  created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_entities_type ON entities(type);
CREATE INDEX idx_entities_canonical ON entities(canonical_name);
CREATE INDEX idx_entities_status ON entities(status);
```

**Rationale.** `slug UNIQUE` is the merge-conflict surface for `INSERT ... ON CONFLICT`. `merged_into_id` self-FK preserves history of operator-confirmed merges; never delete a merged entity. `metadata_json` keeps type-specific fields out of the column list (DOB for persons, EIN for orgs, jurisdiction for boards).

### `aliases`

Variant spellings, DBA names, prior names. Original casing is **always** preserved.

```sql
CREATE TABLE aliases (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  entity_id         INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
  alias             TEXT NOT NULL,                          -- raw, original casing
  source_vault_path TEXT,
  confidence        REAL NOT NULL DEFAULT 0.5
                      CHECK(confidence BETWEEN 0.0 AND 1.0),
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  UNIQUE(entity_id, alias)
);
CREATE INDEX idx_aliases_alias ON aliases(alias);
```

### `relationships`

Typed edges. Time-bounded — a 2018–2020 board member is not a 2024 board member.

```sql
CREATE TABLE relationships (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  from_id           INTEGER NOT NULL REFERENCES entities(id),
  to_id             INTEGER NOT NULL REFERENCES entities(id),
  rel_type          TEXT NOT NULL,                          -- principal_of | board_member_of | spouse_of | donor_to | employed_by | parent_of_org
  start_date        TEXT,                                   -- ISO-8601 or null
  end_date          TEXT,
  source_vault_path TEXT NOT NULL,
  confidence        REAL NOT NULL DEFAULT 0.5
                      CHECK(confidence BETWEEN 0.0 AND 1.0),
  notes             TEXT,
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_rel_from ON relationships(from_id);
CREATE INDEX idx_rel_to ON relationships(to_id);
CREATE INDEX idx_rel_type ON relationships(rel_type);
```

### `transactions`

Money moving between entities at a date.

```sql
CREATE TABLE transactions (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  txn_date          TEXT NOT NULL,                          -- ISO-8601
  from_id           INTEGER REFERENCES entities(id),        -- payer
  to_id             INTEGER REFERENCES entities(id),        -- payee
  amount_usd        REAL,                                   -- null if source unparseable; raw string in description
  txn_type          TEXT NOT NULL,                          -- award | payment | reimbursement | grant | donation | fee
  contract_ref      TEXT,
  rfp_id            INTEGER REFERENCES entities(id),
  is_no_bid         INTEGER,                                -- 0/1/null (unknown)
  description       TEXT,
  source_vault_path TEXT NOT NULL,
  confidence        REAL NOT NULL DEFAULT 0.5
                      CHECK(confidence BETWEEN 0.0 AND 1.0),
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_txn_date ON transactions(txn_date);
CREATE INDEX idx_txn_from ON transactions(from_id);
CREATE INDEX idx_txn_to ON transactions(to_id);
CREATE INDEX idx_txn_type ON transactions(txn_type);
CREATE INDEX idx_txn_no_bid ON transactions(is_no_bid) WHERE is_no_bid = 1;
```

### `events`

Discrete actions: votes, meetings, sponsorships.

```sql
CREATE TABLE events (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  event_date        TEXT NOT NULL,
  event_type        TEXT NOT NULL,                          -- vote | meeting | sponsorship | appointment | filing | hearing
  entity_id         INTEGER REFERENCES entities(id),
  related_ids       TEXT,                                   -- JSON array of entity ids
  payload_json      TEXT,
  source_vault_path TEXT NOT NULL,
  confidence        REAL NOT NULL DEFAULT 0.5
                      CHECK(confidence BETWEEN 0.0 AND 1.0),
  created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX idx_events_date ON events(event_date);
CREATE INDEX idx_events_type ON events(event_type);
CREATE INDEX idx_events_entity ON events(entity_id);
```

### `methodology`

The public transparency artifact. Immutable after insert.

```sql
CREATE TABLE methodology (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  run_date        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  query_label     TEXT NOT NULL,
  sql             TEXT NOT NULL,
  row_count       INTEGER,
  result_hash     TEXT,                                     -- sha256 of canonicalized rows
  asked_by        TEXT NOT NULL,                            -- agent or operator handle
  used_in         TEXT,                                     -- finding slug, populated post-hoc
  notes           TEXT
);
CREATE INDEX idx_methodology_label ON methodology(query_label);
CREATE INDEX idx_methodology_used_in ON methodology(used_in);
```

### `sources`

Bridge to the vault manifest. One row per distinct vault file referenced by a fact row.

```sql
CREATE TABLE sources (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  vault_path      TEXT NOT NULL UNIQUE,
  sha256          TEXT NOT NULL,
  first_referenced TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  kind            TEXT                                       -- pdf | html | data | transcript | image
);
CREATE INDEX idx_sources_sha ON sources(sha256);
```

`source_vault_path` columns on fact tables match `sources.vault_path` by convention; not enforced as FK because vault files are added by Archivist on a separate cadence and we don't want fact upserts to fail just because the sources row hasn't been backfilled yet.

## Design choices, briefly

- **TEXT for dates, not native types.** SQLite's date types are TEXT under the hood; we standardize on ISO-8601 explicitly so string comparisons sort correctly.
- **`metadata_json` over wide tables.** Type-specific fields go in JSON; the `entities` table stays narrow and indexable.
- **Confidence is mandatory.** Every fact row carries one. Default 0.5 (auto-extracted). Operator bump to 0.95+ is the manual override.
- **No FKs from fact tables to `sources`.** Decouples Archivist's ingest pace from Data Reporter's upsert pace.
- **Indexes are conservative.** Add only those that name-normalization queries and the `repeat_winners`/`no_bid_awards` common queries actually use. Re-evaluate with `EXPLAIN QUERY PLAN` when adding watches.

## Migrations

`init_db.py` keeps a list of `migrations: List[Callable[[sqlite3.Connection], None]]`. Each migration:
1. Runs its SQL inside a transaction.
2. Bumps `PRAGMA user_version` by one.

Never edit a past migration; only append. Never `ALTER TABLE` on prod outside a migration.
