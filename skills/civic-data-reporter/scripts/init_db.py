#!/usr/bin/env python3
"""
init_db.py — idempotent schema creator + migration runner for tampa.db.

Usage:
    python init_db.py <db_path>

Emits:
    OK                       — already at current version, no changes
    MIGRATED <from> -> <to>  — schema bumped from <from> to <to>

Exits non-zero on any failure. Stdlib only.
"""
from __future__ import annotations

import sqlite3
import sys
from typing import Callable, List


# --- Migrations ---------------------------------------------------------------
# Each migration takes a connection, runs its DDL, and bumps user_version.
# Append-only: never edit a past migration.

def migration_001_initial(conn: sqlite3.Connection) -> None:
    conn.executescript("""
    CREATE TABLE IF NOT EXISTS entities (
      id              INTEGER PRIMARY KEY AUTOINCREMENT,
      type            TEXT NOT NULL CHECK(type IN ('person','org','contractor','dept','board','rfp','project')),
      canonical_name  TEXT NOT NULL,
      slug            TEXT NOT NULL UNIQUE,
      wiki_path       TEXT,
      first_seen      TEXT NOT NULL,
      last_seen       TEXT NOT NULL,
      status          TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','archived','merged')),
      merged_into_id  INTEGER REFERENCES entities(id),
      metadata_json   TEXT,
      confidence      REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0.0 AND 1.0),
      created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(type);
    CREATE INDEX IF NOT EXISTS idx_entities_canonical ON entities(canonical_name);
    CREATE INDEX IF NOT EXISTS idx_entities_status ON entities(status);

    CREATE TABLE IF NOT EXISTS aliases (
      id                INTEGER PRIMARY KEY AUTOINCREMENT,
      entity_id         INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
      alias             TEXT NOT NULL,
      source_vault_path TEXT,
      confidence        REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0.0 AND 1.0),
      created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
      UNIQUE(entity_id, alias)
    );
    CREATE INDEX IF NOT EXISTS idx_aliases_alias ON aliases(alias);

    CREATE TABLE IF NOT EXISTS relationships (
      id                INTEGER PRIMARY KEY AUTOINCREMENT,
      from_id           INTEGER NOT NULL REFERENCES entities(id),
      to_id             INTEGER NOT NULL REFERENCES entities(id),
      rel_type          TEXT NOT NULL,
      start_date        TEXT,
      end_date          TEXT,
      source_vault_path TEXT NOT NULL,
      confidence        REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0.0 AND 1.0),
      notes             TEXT,
      created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX IF NOT EXISTS idx_rel_from ON relationships(from_id);
    CREATE INDEX IF NOT EXISTS idx_rel_to ON relationships(to_id);
    CREATE INDEX IF NOT EXISTS idx_rel_type ON relationships(rel_type);

    CREATE TABLE IF NOT EXISTS transactions (
      id                INTEGER PRIMARY KEY AUTOINCREMENT,
      txn_date          TEXT NOT NULL,
      from_id           INTEGER REFERENCES entities(id),
      to_id             INTEGER REFERENCES entities(id),
      amount_usd        REAL,
      txn_type          TEXT NOT NULL,
      contract_ref      TEXT,
      rfp_id            INTEGER REFERENCES entities(id),
      is_no_bid         INTEGER,
      description       TEXT,
      source_vault_path TEXT NOT NULL,
      confidence        REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0.0 AND 1.0),
      created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX IF NOT EXISTS idx_txn_date ON transactions(txn_date);
    CREATE INDEX IF NOT EXISTS idx_txn_from ON transactions(from_id);
    CREATE INDEX IF NOT EXISTS idx_txn_to ON transactions(to_id);
    CREATE INDEX IF NOT EXISTS idx_txn_type ON transactions(txn_type);
    CREATE INDEX IF NOT EXISTS idx_txn_no_bid ON transactions(is_no_bid) WHERE is_no_bid = 1;

    CREATE TABLE IF NOT EXISTS events (
      id                INTEGER PRIMARY KEY AUTOINCREMENT,
      event_date        TEXT NOT NULL,
      event_type        TEXT NOT NULL,
      entity_id         INTEGER REFERENCES entities(id),
      related_ids       TEXT,
      payload_json      TEXT,
      source_vault_path TEXT NOT NULL,
      confidence        REAL NOT NULL DEFAULT 0.5 CHECK(confidence BETWEEN 0.0 AND 1.0),
      created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX IF NOT EXISTS idx_events_date ON events(event_date);
    CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
    CREATE INDEX IF NOT EXISTS idx_events_entity ON events(entity_id);

    CREATE TABLE IF NOT EXISTS methodology (
      id              INTEGER PRIMARY KEY AUTOINCREMENT,
      run_date        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
      query_label     TEXT NOT NULL,
      sql             TEXT NOT NULL,
      row_count       INTEGER,
      result_hash     TEXT,
      asked_by        TEXT NOT NULL,
      used_in         TEXT,
      notes           TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_methodology_label ON methodology(query_label);
    CREATE INDEX IF NOT EXISTS idx_methodology_used_in ON methodology(used_in);

    CREATE TABLE IF NOT EXISTS sources (
      id              INTEGER PRIMARY KEY AUTOINCREMENT,
      vault_path      TEXT NOT NULL UNIQUE,
      sha256          TEXT NOT NULL,
      first_referenced TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
      kind            TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_sources_sha ON sources(sha256);
    """)


MIGRATIONS: List[Callable[[sqlite3.Connection], None]] = [
    migration_001_initial,
]
TARGET_VERSION = len(MIGRATIONS)


def main(argv: List[str]) -> int:
    if len(argv) != 2:
        print("usage: init_db.py <db_path>", file=sys.stderr)
        return 2
    db_path = argv[1]

    conn = sqlite3.connect(db_path)
    try:
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        cur = conn.execute("PRAGMA user_version")
        current = cur.fetchone()[0]

        if current > TARGET_VERSION:
            print(f"ERROR: db version {current} > target {TARGET_VERSION}", file=sys.stderr)
            return 3

        if current == TARGET_VERSION:
            print("OK")
            return 0

        start = current
        for i in range(current, TARGET_VERSION):
            with conn:
                MIGRATIONS[i](conn)
                conn.execute(f"PRAGMA user_version = {i + 1}")
        print(f"MIGRATED {start} -> {TARGET_VERSION}")
        return 0
    finally:
        conn.close()


if __name__ == "__main__":
    sys.exit(main(sys.argv))
